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
//! The buffer remembers which bytes were typed and which arrived inside which
//! paste, and a submitted prompt carries that split (a [`Submission`]): recent
//! Claude Code builds wrap a long paste in a `<pasted_content>` tag before
//! submitting it, and a scenario can opt into mirroring that (see
//! [`crate::pasted_content`]).
//!
//! The terminal must be in raw mode for this to work: in canonical mode the
//! kernel line-buffers stdin and a lone Escape would never be delivered.
//!
//! The byte-level state machine lives in `decode`, and the stdin reader
//! thread that feeds it (with the escape-time timeout) in `reader`.

mod decode;
mod line_buffer;
mod prompt_part;
mod reader;
mod submission;
#[cfg(test)]
mod test_support;

pub use prompt_part::PromptPart;
pub use reader::spawn_reader;
pub use submission::Submission;

/// One user-level input event decoded from the raw byte stream.
#[derive(Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// A line of text was submitted (Enter on a non-empty buffer).
    Prompt(Submission),
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
