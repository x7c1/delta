//! The stdin reader: a thread copying raw bytes off stdin, and the decoder
//! loop that feeds them through [`step`] with an escape-time timeout.

use std::io::Read;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use super::decode::{step, Mode};
use super::line_buffer::LineBuffer;
use super::InputEvent;

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
    let mut buffer = LineBuffer::default();
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
                    buffer.push_pasted(0x1b);
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

#[cfg(test)]
mod tests {
    use super::super::test_support::Decoded;
    use super::*;

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
        assert_eq!(Decoded::from(event), Decoded::Interrupt);
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
        assert_eq!(Decoded::from(event), Decoded::Prompt("hi".to_owned()));
        // No further events: the BPM markers were consumed and no
        // spurious Interrupt was emitted along the way.
        assert!(events.recv_timeout(TEST_ESCAPE_TIMEOUT * 2).is_err());
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
        assert_eq!(Decoded::from(event), Decoded::Prompt("a\x1bz".to_owned()));
    }
}
