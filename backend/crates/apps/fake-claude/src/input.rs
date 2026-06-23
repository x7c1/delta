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
use std::sync::mpsc::{channel, Receiver, Sender};

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

/// Spawn the stdin reader thread, returning the decoded event stream.
pub fn spawn_reader() -> Receiver<InputEvent> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        decode_stream(stdin.lock(), &tx);
    });
    rx
}

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
}
