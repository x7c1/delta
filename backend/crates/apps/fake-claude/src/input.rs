//! Reading pane input the way tmux delivers it.
//!
//! Delta types into the pane with `tmux send-keys`: a clear sequence (`C-u`
//! then a run of `BSpace`), the literal message text wrapped in xterm
//! bracketed-paste markers (`ESC [ 200 ~` … `ESC [ 201 ~`), and — after a
//! settle — a lone `Enter`. A human attached through the embedded terminal
//! produces the same raw byte stream (terminal emulators wrap real paste
//! events the same way). So the fake's "TUI input box" is a byte-level line
//! editor over stdin:
//!
//! - `0x15` (`C-u`) clears the pending buffer,
//! - `0x7f`/`0x08` (backspace) deletes one byte,
//! - `0x0d` (Enter) submits the buffer as a prompt (an empty submit is
//!   ignored, mirroring a TUI ignoring Enter on an empty input),
//! - `0x1b` (Escape) starts a CSI scan that may resolve to the
//!   bracketed-paste start marker; an Escape that is not part of a CSI
//!   resolves to an interrupt,
//! - anything else accumulates into the buffer.
//!
//! Inside a bracketed-paste region every byte is accumulated verbatim
//! (including `0x0a` LF, `0x0d` CR, `0x15` C-u, lone `0x1b` ESC) until the
//! paired `ESC [ 201 ~` end marker arrives — mirroring real Claude's TUI,
//! which consumes the markers and stores the inner bytes literally.
//!
//! The terminal must be in raw mode for this to work: in canonical mode the
//! kernel line-buffers stdin and a lone Escape would never be delivered.

use std::io::Read;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

/// One user-level input event decoded from the raw byte stream.
#[derive(Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// A line of text was submitted (Enter on a non-empty buffer).
    Prompt(String),
    /// Escape was pressed.
    Interrupt,
}

/// Put the controlling terminal into raw mode for the life of the process.
///
/// Errors are reported but not fatal: when stdin is not a tty (running the
/// fake outside tmux, e.g. in a pipe-driven test) line-based input still
/// works, only the single-byte keys lose their immediacy.
pub fn enable_raw_mode() {
    // SAFETY: plain libc termios calls on fd 0, with a zeroed struct the
    // kernel fills; no aliasing or lifetime concerns.
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) != 0 {
            eprintln!("fake-claude: stdin is not a tty; raw mode skipped");
            return;
        }
        libc::cfmakeraw(&mut termios);
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &termios) != 0 {
            eprintln!("fake-claude: failed to enter raw mode");
        }
    }
}

/// Spawn the stdin reader, returning the decoded event stream.
///
/// Wired as a two-stage pipeline so the decoder can disambiguate a lone ESC
/// from the start of a CSI sequence via a small **escape-time timeout**
/// without coupling the byte read loop to the decoder's wait semantics:
///
/// 1. A blocking reader thread copies raw stdin bytes into an mpsc channel.
/// 2. A decoder thread consumes that channel through [`decode_with_timeout`],
///    which uses [`recv_timeout`] only while the state machine is waiting on
///    the byte after an ESC. On timeout, a lone ESC resolves as
///    [`InputEvent::Interrupt`] (matching how real terminals disambiguate
///    ESC via the "escape-time" setting), and an ESC inside a paste flushes
///    as a literal content byte and stays in paste mode. All other states
///    [`recv`] without a timeout, so the decoder still blocks on real input.
///
/// Why this is necessary: delta injects Cancel as a lone `Escape` keystroke
/// via `tmux send-keys ... Escape`, so the byte stream ends with `0x1b`
/// followed by nothing. Without a timeout the decoder would sit in
/// [`Mode::EscSeen`] forever waiting for a follow-up byte that never comes,
/// and the `AwaitEscape` scenario step would never unblock.
///
/// [`recv`]: Receiver::recv
/// [`recv_timeout`]: Receiver::recv_timeout
pub fn spawn_reader() -> Receiver<InputEvent> {
    let (byte_tx, byte_rx) = channel::<u8>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut handle = stdin.lock();
        let mut byte = [0u8; 1];
        while let Ok(1) = handle.read(&mut byte) {
            if byte_tx.send(byte[0]).is_err() {
                return;
            }
        }
    });
    let (event_tx, event_rx) = channel::<InputEvent>();
    std::thread::spawn(move || decode_with_timeout(&byte_rx, &event_tx, ESCAPE_TIMEOUT));
    event_rx
}

/// How long the decoder waits for a follow-up byte after seeing ESC before
/// resolving the ESC as a lone keystroke (an [`InputEvent::Interrupt`]
/// outside a paste, a literal `0x1b` byte inside one).
///
/// Real terminals disambiguate ESC the same way (vanilla tmux's `escape-time`
/// is 500ms; delta's pinned tmux config sets it to 0 because the xterm.js
/// bridge always delivers escape sequences as a single write). tmux's
/// `send-keys -l` likewise emits the payload as one write, so adjacent bytes
/// of a CSI sequence arrive microseconds apart. 50ms is generous margin
/// against scheduling jitter while staying well under any human-perceptible
/// delay on a lone Escape.
const ESCAPE_TIMEOUT: Duration = Duration::from_millis(50);

/// Tracks where in the byte stream the decoder is so each byte can be
/// classified correctly even when CSI sequences and bracketed-paste regions
/// arrive byte-by-byte. tmux's `send-keys -l` writes the payload through the
/// pane in 1-byte reads, so the decoder cannot peek ahead.
enum Mode {
    /// Normal line-editor mode: byte commands fire as documented at the top
    /// of this module.
    Normal,
    /// Saw `0x1b` outside paste mode; the next byte decides whether this is
    /// an interrupt (anything that is not `[`) or the start of a CSI scan.
    EscSeen,
    /// Saw `ESC [` outside paste mode; collecting the parameter bytes until
    /// the final `~`. Only one terminator is recognized: `200~` enters paste
    /// mode. Anything else is ignored (no other CSI sequence is meaningful
    /// to this line editor).
    CsiSeen { params: Vec<u8> },
    /// Inside a bracketed-paste region: every byte accumulates verbatim
    /// (including LF, CR, C-u, lone ESC) until the paired end marker.
    Pasting,
    /// Saw `0x1b` inside paste mode; might be the start of the
    /// `ESC [ 201 ~` end marker, or might just be a literal ESC byte in the
    /// pasted content.
    PastingEscSeen,
    /// Saw `ESC [` inside paste mode; collecting parameter bytes until the
    /// final `~`. Only `201~` exits paste mode; anything else (including a
    /// stray `200~` inside the paste) is treated as literal content and
    /// flushed back into the buffer.
    PastingCsiSeen { params: Vec<u8> },
}

/// Decode the raw byte stream into [`InputEvent`]s until EOF.
///
/// Synchronous, byte-by-byte path used by the unit tests against a `&[u8]`
/// fixture. The production reader uses [`decode_with_timeout`] instead,
/// because real input arrives over a channel where EOF and "no byte yet"
/// are distinguishable and a lone ESC has to resolve on a timeout — not
/// only at EOF. Kept as a separate entry point (and gated behind `cfg(test)`
/// since production never calls it) so the deterministic state-machine tests
/// — no timing, no threads — stay easy to read.
#[cfg(test)]
fn decode_stream(mut reader: impl Read, events: &Sender<InputEvent>) {
    let mut buffer: Vec<u8> = Vec::new();
    let mut mode = Mode::Normal;
    let mut byte = [0u8; 1];
    while let Ok(1) = reader.read(&mut byte) {
        let mut produced: Vec<InputEvent> = Vec::new();
        step(&mut mode, &mut buffer, byte[0], &mut produced);
        for event in produced {
            if events.send(event).is_err() {
                return; // The engine hung up; nothing left to deliver to.
            }
        }
    }
    // EOF reached. If a lone ESC was buffered waiting for its follow-up byte
    // to decide between "interrupt" and "CSI start", resolve it as the
    // interrupt it stood for — terminating mid-CSI/mid-paste is treated as a
    // dropped sequence and the in-flight bytes are discarded (matching the
    // pre-bracketed-paste behavior where a lone ESC immediately emitted an
    // interrupt).
    if matches!(mode, Mode::EscSeen) {
        let _ = events.send(InputEvent::Interrupt);
    }
}

/// Decode bytes from a channel, using [`recv_timeout`] only while the state
/// machine is waiting for the follow-up byte after an ESC, so a lone ESC
/// resolves promptly instead of blocking forever on input that will never
/// arrive (see [`spawn_reader`] for the why). Runs until the byte channel is
/// disconnected (the reader thread saw EOF on stdin).
///
/// `escape_timeout` is injected so tests can drive the timeout path with a
/// short window without relying on the production [`ESCAPE_TIMEOUT`].
///
/// [`recv_timeout`]: Receiver::recv_timeout
fn decode_with_timeout(
    bytes: &Receiver<u8>,
    events: &Sender<InputEvent>,
    escape_timeout: Duration,
) {
    let mut buffer: Vec<u8> = Vec::new();
    let mut mode = Mode::Normal;
    loop {
        let waiting_for_csi = matches!(mode, Mode::EscSeen | Mode::PastingEscSeen);
        let next = if waiting_for_csi {
            bytes.recv_timeout(escape_timeout)
        } else {
            bytes.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match next {
            Ok(byte) => {
                let mut produced: Vec<InputEvent> = Vec::new();
                step(&mut mode, &mut buffer, byte, &mut produced);
                for event in produced {
                    if events.send(event).is_err() {
                        return;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => match mode {
                Mode::EscSeen => {
                    if events.send(InputEvent::Interrupt).is_err() {
                        return;
                    }
                    mode = Mode::Normal;
                }
                Mode::PastingEscSeen => {
                    // The ESC was a literal byte in the pasted payload with
                    // no follow-up CSI; flush it back into the buffer and
                    // keep collecting paste content until `ESC [ 201 ~`.
                    buffer.push(0x1b);
                    mode = Mode::Pasting;
                }
                // Unreachable: `waiting_for_csi` is only true in the two
                // arms above, and the `recv` branch never returns Timeout.
                _ => {}
            },
            Err(RecvTimeoutError::Disconnected) => {
                // Byte channel closed — reader thread saw EOF. Mirror
                // `decode_stream`'s EOF behavior: a lone ESC stranded in
                // `EscSeen` resolves as an Interrupt; in-flight CSI/paste
                // bytes are dropped.
                if matches!(mode, Mode::EscSeen) {
                    let _ = events.send(InputEvent::Interrupt);
                }
                return;
            }
        }
    }
}

/// Advance the decoder by one byte, appending any events the byte produced
/// to `produced`.
///
/// Pulled out as a free function so the state transitions are testable in
/// isolation and the read loop in [`decode_stream`] stays a thin wrapper.
/// A single byte can produce multiple events when a deferred decision
/// resolves: e.g. `ESC` followed by `\r` on a non-empty buffer emits both an
/// `Interrupt` (the ESC stood alone) and a `Prompt` (the `\r` submits the
/// already-typed buffer).
fn step(mode: &mut Mode, buffer: &mut Vec<u8>, byte: u8, produced: &mut Vec<InputEvent>) {
    match mode {
        Mode::Normal => match byte {
            0x15 => {
                buffer.clear();
            }
            0x7f | 0x08 => {
                buffer.pop();
            }
            0x0d => {
                if !buffer.is_empty() {
                    let text = String::from_utf8_lossy(buffer).into_owned();
                    buffer.clear();
                    produced.push(InputEvent::Prompt(text));
                }
            }
            0x1b => {
                // Defer the interrupt decision: this might be the start of a
                // bracketed-paste CSI. If the next byte is not `[`, the ESC
                // is resolved as an interrupt then.
                *mode = Mode::EscSeen;
            }
            other => {
                buffer.push(other);
            }
        },
        Mode::EscSeen => {
            if byte == b'[' {
                *mode = Mode::CsiSeen { params: Vec::new() };
            } else {
                // The ESC stood alone, so it really was an interrupt; the
                // current byte is the next instruction and is re-fed through
                // the decoder so a `\r` after an Escape still submits, etc.
                *mode = Mode::Normal;
                produced.push(InputEvent::Interrupt);
                // Bounded recursion: `Normal` only re-enters `EscSeen` on a
                // fresh ESC, which itself needs another byte before it can
                // recurse — so this terminates after at most one re-step.
                step(mode, buffer, byte, produced);
            }
        }
        Mode::CsiSeen { params } => {
            if byte == b'~' {
                let entering_paste = params.as_slice() == b"200";
                *mode = if entering_paste {
                    Mode::Pasting
                } else {
                    Mode::Normal
                };
            } else {
                params.push(byte);
            }
        }
        Mode::Pasting => match byte {
            0x1b => {
                *mode = Mode::PastingEscSeen;
            }
            other => {
                buffer.push(other);
            }
        },
        Mode::PastingEscSeen => {
            if byte == b'[' {
                *mode = Mode::PastingCsiSeen { params: Vec::new() };
            } else {
                // The ESC was a literal byte in the pasted payload (e.g. a
                // user pasted a raw control sequence). Flush the ESC back
                // into the buffer, then re-process the current byte under
                // paste mode so its semantics (another ESC, an LF, …) are
                // preserved.
                buffer.push(0x1b);
                *mode = Mode::Pasting;
                step(mode, buffer, byte, produced);
            }
        }
        Mode::PastingCsiSeen { params } => {
            if byte == b'~' {
                if params.as_slice() == b"201" {
                    // Paired end marker: exit paste mode without storing
                    // the marker bytes.
                    *mode = Mode::Normal;
                } else {
                    // Not the end marker. Real Claude treats any non-201
                    // CSI inside a paste as literal content (paste mode
                    // doesn't re-enter on a nested `200~`), so flush the
                    // collected bytes back into the buffer verbatim.
                    buffer.push(0x1b);
                    buffer.push(b'[');
                    buffer.extend_from_slice(params);
                    buffer.push(b'~');
                    *mode = Mode::Pasting;
                }
            } else {
                params.push(byte);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> Vec<InputEvent> {
        let (tx, rx) = channel();
        decode_stream(bytes, &tx);
        drop(tx);
        rx.into_iter().collect()
    }

    #[test]
    fn a_typed_line_submits_on_enter() {
        assert_eq!(
            decode(b"hello\r"),
            vec![InputEvent::Prompt("hello".to_owned())]
        );
    }

    #[test]
    fn the_tmux_clear_sequence_leaves_the_buffer_empty() {
        // C-u, a burst of BSpace (deleting past the start is a no-op), then the
        // message and its delayed Enter — exactly what `send_line` produces.
        let mut bytes = vec![0x15];
        bytes.extend(std::iter::repeat_n(0x7f, 64));
        bytes.extend(b"next message\r");
        assert_eq!(
            decode(&bytes),
            vec![InputEvent::Prompt("next message".to_owned())]
        );
    }

    #[test]
    fn escape_is_an_interrupt() {
        assert_eq!(decode(b"\x1b"), vec![InputEvent::Interrupt]);
    }

    #[test]
    fn enter_on_an_empty_buffer_is_ignored() {
        assert_eq!(decode(b"\r\r"), Vec::<InputEvent>::new());
    }

    #[test]
    fn embedded_newlines_stay_in_the_message() {
        assert_eq!(
            decode(b"line one\nline two\r"),
            vec![InputEvent::Prompt("line one\nline two".to_owned())]
        );
    }

    #[test]
    fn bracketed_paste_markers_are_consumed_around_a_single_line() {
        // The start/end markers themselves are not stored in the buffer;
        // only the inner bytes reach the prompt event.
        assert_eq!(
            decode(b"\x1b[200~hello\x1b[201~\r"),
            vec![InputEvent::Prompt("hello".to_owned())]
        );
    }

    #[test]
    fn bracketed_paste_preserves_embedded_lf() {
        // The regression: real Claude's TUI normalizes LF to space outside
        // paste mode, so the fake must consume LF verbatim inside a paste —
        // otherwise e2e tests would pass on the fake while the real TUI
        // would still mangle the bytes. This is the LF half of the fix.
        assert_eq!(
            decode(b"\x1b[200~line one\nline two\x1b[201~\r"),
            vec![InputEvent::Prompt("line one\nline two".to_owned())]
        );
    }

    #[test]
    fn paste_mode_suppresses_byte_level_commands() {
        // C-u (0x15) and CR (0x0d) inside a paste are content bytes, not
        // commands. The buffer must NOT be cleared and Enter must NOT submit
        // until after the paste end marker arrives.
        assert_eq!(
            decode(b"\x1b[200~keep\x15keep\rkeep\x1b[201~\r"),
            vec![InputEvent::Prompt("keep\x15keep\rkeep".to_owned())]
        );
    }

    #[test]
    fn nested_paste_start_inside_a_paste_is_treated_as_literal_text() {
        // Real Claude's TUI does not re-enter paste mode on a second `200~`
        // inside an open paste; only `201~` exits. The inner marker bytes
        // are stored verbatim, then the outer paste closes on `201~`.
        assert_eq!(
            decode(b"\x1b[200~outer\x1b[200~still outer\x1b[201~\r"),
            vec![InputEvent::Prompt(
                "outer\x1b[200~still outer".to_owned()
            )]
        );
    }

    #[test]
    fn the_real_tmux_send_line_sequence_decodes_to_one_prompt() {
        // End-to-end byte stream that real `send_line` produces for a
        // multi-line prompt: clear (C-u + a run of BSpace) → BPM-wrapped
        // payload → Enter. The decoder must surface exactly one Prompt
        // with the embedded LF preserved.
        let mut bytes = vec![0x15];
        bytes.extend(std::iter::repeat_n(0x7f, 64));
        bytes.extend(b"\x1b[200~line one\nline two\x1b[201~");
        bytes.extend(b"\r");
        assert_eq!(
            decode(&bytes),
            vec![InputEvent::Prompt("line one\nline two".to_owned())]
        );
    }

    #[test]
    fn escape_inside_paste_is_a_literal_byte_not_an_interrupt() {
        // A lone ESC inside paste mode is content, not an interrupt — only
        // an exit marker `ESC [ 201 ~` closes the paste. The standalone ESC
        // (followed by a non-`[` byte) is flushed back into the buffer.
        assert_eq!(
            decode(b"\x1b[200~a\x1bz\x1b[201~\r"),
            vec![InputEvent::Prompt("a\x1bz".to_owned())]
        );
    }

    /// Test-only escape timeout. Short enough to keep the suite fast, long
    /// enough that a "send all bytes synchronously" feed never trips it on a
    /// loaded CI runner.
    const TEST_ESCAPE_TIMEOUT: Duration = Duration::from_millis(30);

    /// Spawn the decoder loop against a fresh pair of channels so the test
    /// can drive bytes in and read events out the way production does.
    fn spawn_decoder_with_timeout() -> (Sender<u8>, Receiver<InputEvent>) {
        let (byte_tx, byte_rx) = channel::<u8>();
        let (event_tx, event_rx) = channel::<InputEvent>();
        std::thread::spawn(move || {
            decode_with_timeout(&byte_rx, &event_tx, TEST_ESCAPE_TIMEOUT);
        });
        (byte_tx, event_rx)
    }

    #[test]
    fn a_lone_escape_resolves_via_the_escape_time_timeout() {
        // The regression: delta's Cancel button injects a single 0x1b via
        // `tmux send-keys ... Escape`. Without a timeout the decoder would
        // sit in EscSeen forever waiting for a follow-up byte that never
        // arrives. With the timeout, ESC resolves as Interrupt.
        let (bytes, events) = spawn_decoder_with_timeout();
        bytes.send(0x1b).unwrap();
        let event = events
            .recv_timeout(TEST_ESCAPE_TIMEOUT * 4)
            .expect("decoder should emit Interrupt after the escape-time timeout");
        assert_eq!(event, InputEvent::Interrupt);
    }

    #[test]
    fn a_full_csi_sequence_does_not_trip_the_escape_time_timeout() {
        // When the bracketed-paste start marker arrives as one tight burst
        // (the only way tmux's send-keys -l emits it), the decoder enters
        // paste mode without ever emitting a stray Interrupt — proving the
        // timeout is gated on actually waiting for a missing byte rather
        // than on simply having seen an ESC.
        let (bytes, events) = spawn_decoder_with_timeout();
        for &b in b"\x1b[200~hi\x1b[201~\r" {
            bytes.send(b).unwrap();
        }
        let event = events
            .recv_timeout(TEST_ESCAPE_TIMEOUT * 4)
            .expect("decoder should still emit the Prompt event");
        assert_eq!(event, InputEvent::Prompt("hi".to_owned()));
        // No further events: the BPM markers were consumed and no
        // spurious Interrupt was emitted along the way.
        assert!(events
            .recv_timeout(TEST_ESCAPE_TIMEOUT * 2)
            .is_err());
    }

    #[test]
    fn an_escape_inside_an_open_paste_is_flushed_on_timeout() {
        // Inside a paste, a lone ESC with no follow-up CSI must be stored
        // as a literal content byte rather than emitting an Interrupt.
        // The paste then closes normally on `ESC [ 201 ~`.
        let (bytes, events) = spawn_decoder_with_timeout();
        for &b in b"\x1b[200~a" {
            bytes.send(b).unwrap();
        }
        // Lone ESC inside paste — wait past the timeout so the decoder
        // resolves it as a literal byte, then close the paste and submit.
        bytes.send(0x1b).unwrap();
        std::thread::sleep(TEST_ESCAPE_TIMEOUT * 2);
        for &b in b"z\x1b[201~\r" {
            bytes.send(b).unwrap();
        }
        let event = events
            .recv_timeout(TEST_ESCAPE_TIMEOUT * 4)
            .expect("decoder should emit the Prompt with the literal ESC kept");
        assert_eq!(event, InputEvent::Prompt("a\x1bz".to_owned()));
    }
}
