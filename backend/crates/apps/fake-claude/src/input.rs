//! Reading pane input the way tmux delivers it.
//!
//! Delta types into the pane with `tmux send-keys`: a clear sequence (`C-u`
//! then a run of `BSpace`), the literal message text, and — after a settle —
//! a lone `Enter`. A human attached through the embedded terminal produces
//! the same raw byte stream. So the fake's "TUI input box" is a byte-level
//! line editor over stdin:
//!
//! - `0x15` (`C-u`) clears the pending buffer,
//! - `0x7f`/`0x08` (backspace) deletes one byte,
//! - `0x0d` (Enter) submits the buffer as a prompt (an empty submit is
//!   ignored, mirroring a TUI ignoring Enter on an empty input),
//! - `0x1b` (Escape) is an interrupt,
//! - anything else (including `0x0a` inside pasted multi-line text)
//!   accumulates into the buffer.
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

/// Decode the raw byte stream into [`InputEvent`]s until EOF.
fn decode_stream(mut reader: impl Read, events: &Sender<InputEvent>) {
    let mut buffer: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    while let Ok(1) = reader.read(&mut byte) {
        let event = match byte[0] {
            0x15 => {
                buffer.clear();
                None
            }
            0x7f | 0x08 => {
                buffer.pop();
                None
            }
            0x0d => {
                if buffer.is_empty() {
                    None
                } else {
                    let text = String::from_utf8_lossy(&buffer).into_owned();
                    buffer.clear();
                    Some(InputEvent::Prompt(text))
                }
            }
            0x1b => Some(InputEvent::Interrupt),
            other => {
                buffer.push(other);
                None
            }
        };
        if let Some(event) = event {
            if events.send(event).is_err() {
                return; // The engine hung up; nothing left to deliver to.
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
}
