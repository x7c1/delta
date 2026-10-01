//! The byte-level decoder: the state machine that turns raw pane bytes into
//! [`InputEvent`]s one byte at a time.

#[cfg(test)]
use std::io::Read;
#[cfg(test)]
use std::sync::mpsc::Sender;

use super::line_buffer::LineBuffer;
use super::InputEvent;

/// Tracks where in the byte stream the decoder is so each byte can be
/// classified correctly even when CSI sequences and bracketed-paste regions
/// arrive byte-by-byte. tmux's `send-keys -l` writes the payload through the
/// pane in 1-byte reads, so the decoder cannot peek ahead.
pub(super) enum Mode {
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
    let mut buffer = LineBuffer::default();
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
/// isolation and both read loops — the production `decode_with_timeout` and
/// the test-only `decode_stream` — stay thin wrappers.
/// A single byte can produce multiple events when a deferred decision
/// resolves: e.g. `ESC` followed by `\r` on a non-empty buffer emits both an
/// `Interrupt` (the ESC stood alone) and a `Prompt` (the `\r` submits the
/// already-typed buffer).
pub(super) fn step(
    mode: &mut Mode,
    buffer: &mut LineBuffer,
    byte: u8,
    produced: &mut Vec<InputEvent>,
) {
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
                    produced.push(InputEvent::Prompt(buffer.take()));
                }
            }
            0x1b => {
                // Defer the interrupt decision: this might be the start of a
                // bracketed-paste CSI. If the next byte is not `[`, the ESC
                // is resolved as an interrupt then.
                *mode = Mode::EscSeen;
            }
            other => {
                buffer.push_typed(other);
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
                    buffer.start_paste();
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
                buffer.push_pasted(other);
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
                buffer.push_pasted(0x1b);
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
                    buffer.push_pasted(0x1b);
                    buffer.push_pasted(b'[');
                    for &param in params.iter() {
                        buffer.push_pasted(param);
                    }
                    buffer.push_pasted(b'~');
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
    use std::sync::mpsc::channel;

    use super::super::test_support::Decoded;
    use super::super::{PromptPart, Submission};
    use super::*;

    fn decode_events(bytes: &[u8]) -> Vec<InputEvent> {
        let (tx, rx) = channel();
        decode_stream(bytes, &tx);
        drop(tx);
        rx.into_iter().collect()
    }

    fn decode(bytes: &[u8]) -> Vec<Decoded> {
        decode_events(bytes)
            .into_iter()
            .map(Decoded::from)
            .collect()
    }

    fn typed(text: &str) -> PromptPart {
        PromptPart {
            text: text.to_owned(),
            pasted: false,
        }
    }

    fn pasted(text: &str) -> PromptPart {
        PromptPart {
            text: text.to_owned(),
            pasted: true,
        }
    }

    #[test]
    fn a_submission_marks_which_runs_were_pasted() {
        assert_eq!(
            decode_events(b"look \x1b[200~pasted\x1b[201~ here\r"),
            vec![InputEvent::Prompt(Submission {
                parts: vec![typed("look "), pasted("pasted"), typed(" here")],
            })]
        );
    }

    #[test]
    fn back_to_back_pastes_stay_separate_parts() {
        assert_eq!(
            decode_events(b"\x1b[200~one\x1b[201~\x1b[200~two\x1b[201~\r"),
            vec![InputEvent::Prompt(Submission {
                parts: vec![pasted("one"), pasted("two")],
            })]
        );
    }

    #[test]
    fn a_typed_line_is_one_typed_part() {
        assert_eq!(
            decode_events(b"hello\r"),
            vec![InputEvent::Prompt(Submission::typed("hello"))]
        );
    }

    #[test]
    fn clearing_the_buffer_forgets_an_earlier_paste() {
        // The tmux clear sequence wipes a leftover paste; only what follows
        // is submitted, with its own origin.
        assert_eq!(
            decode_events(b"\x1b[200~stale\x1b[201~\x15fresh\r"),
            vec![InputEvent::Prompt(Submission::typed("fresh"))]
        );
    }

    #[test]
    fn a_typed_line_submits_on_enter() {
        assert_eq!(
            decode(b"hello\r"),
            vec![Decoded::Prompt("hello".to_owned())]
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
            vec![Decoded::Prompt("next message".to_owned())]
        );
    }

    #[test]
    fn escape_is_an_interrupt() {
        assert_eq!(decode(b"\x1b"), vec![Decoded::Interrupt]);
    }

    #[test]
    fn enter_on_an_empty_buffer_is_ignored() {
        assert_eq!(decode(b"\r\r"), Vec::<Decoded>::new());
    }

    #[test]
    fn embedded_newlines_stay_in_the_message() {
        assert_eq!(
            decode(b"line one\nline two\r"),
            vec![Decoded::Prompt("line one\nline two".to_owned())]
        );
    }

    #[test]
    fn bracketed_paste_markers_are_consumed_around_a_single_line() {
        // The start/end markers themselves are not stored in the buffer;
        // only the inner bytes reach the prompt event.
        assert_eq!(
            decode(b"\x1b[200~hello\x1b[201~\r"),
            vec![Decoded::Prompt("hello".to_owned())]
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
            vec![Decoded::Prompt("line one\nline two".to_owned())]
        );
    }

    #[test]
    fn paste_mode_suppresses_byte_level_commands() {
        // C-u (0x15) and CR (0x0d) inside a paste are content bytes, not
        // commands. The buffer must NOT be cleared and Enter must NOT submit
        // until after the paste end marker arrives.
        assert_eq!(
            decode(b"\x1b[200~keep\x15keep\rkeep\x1b[201~\r"),
            vec![Decoded::Prompt("keep\x15keep\rkeep".to_owned())]
        );
    }

    #[test]
    fn nested_paste_start_inside_a_paste_is_treated_as_literal_text() {
        // Real Claude's TUI does not re-enter paste mode on a second `200~`
        // inside an open paste; only `201~` exits. The inner marker bytes
        // are stored verbatim, then the outer paste closes on `201~`.
        assert_eq!(
            decode(b"\x1b[200~outer\x1b[200~still outer\x1b[201~\r"),
            vec![Decoded::Prompt("outer\x1b[200~still outer".to_owned())]
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
            vec![Decoded::Prompt("line one\nline two".to_owned())]
        );
    }

    #[test]
    fn escape_inside_paste_is_a_literal_byte_not_an_interrupt() {
        // A lone ESC inside paste mode is content, not an interrupt — only
        // an exit marker `ESC [ 201 ~` closes the paste. The standalone ESC
        // (followed by a non-`[` byte) is flushed back into the buffer.
        assert_eq!(
            decode(b"\x1b[200~a\x1bz\x1b[201~\r"),
            vec![Decoded::Prompt("a\x1bz".to_owned())]
        );
    }
}
