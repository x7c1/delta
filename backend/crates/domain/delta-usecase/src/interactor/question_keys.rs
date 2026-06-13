//! The pure key-sequence generator for answering an `AskUserQuestion` prompt by
//! injecting keystrokes into the live `claude` TUI.
//!
//! A CLI hook cannot return the user's selection, so the only way to answer an
//! `AskUserQuestion` from outside the TUI is to drive its on-screen widget the
//! way a human would: move the highlight and press the keys that record the
//! choice. That coupling to the TUI's exact layout and key handling is fragile,
//! so it is isolated here in one pure, unit-tested function — every other layer
//! (the actor, the tmux driver) only forwards the [`Key`]s this produces.
//!
//! ## Pinned TUI behavior (claude v2.1.177)
//!
//! The sequences below were verified empirically against a real `claude`. The
//! `AskUserQuestion` widget lists the real options `1..N` with the highlight on
//! option 1 by default; any trailing non-answer rows ("Type something", "Chat
//! about this") sit *after* the real options, so navigating `Down` from the top
//! by an option index never lands on them. The generator only ever moves within
//! `0..option_count`, so those trailing rows are structurally unreachable.
//!
//! - **Single question, single-select** — pick option `i`: `Down`×`i`, then
//!   `Enter`. `Enter` on an option submits immediately.
//! - **Single question, multi-select** — each option shows `[ ]`. For each
//!   selected option, move the highlight to it (`Down`/`Up` by the delta from
//!   the current position) and press `Space` to toggle it on. After toggling
//!   every selection, `Right` moves to the Submit tab and `Enter` submits.
//!   `Enter` on an option only toggles in multi-select, so it is never used to
//!   submit here.
//! - **Multiple questions** (tabbed `[Q1][Q2]…[Submit]`) — each single-select
//!   question's `Down`×`i` + `Enter` records it and auto-advances to the next
//!   question's tab. After the last question is recorded a "Review your answers"
//!   screen appears with the default on `1. Submit answers`, so one final
//!   `Enter` submits.
//!
//! Multi-select *within* a multi-question call is deliberately not generated:
//! its advance key (`Space` to toggle, then `Right`/`Tab` to the next tab) was
//! not part of the verified probe, so [`answer_keys`] rejects it rather than
//! emit a guessed sequence that could mis-answer. See [`QuestionKeyError`].

/// One keystroke to inject into the TUI, named as the tmux `send-keys` key the
/// gateway forwards it as.
///
/// The vocabulary is exactly what the pinned `AskUserQuestion` sequences need:
/// `Down`/`Up` to move the highlight, `Space` to toggle a multi-select option,
/// `Right` to reach the Submit tab, and `Enter` to record/submit. (`Tab` would
/// be the multi-select-within-multi-question advance key, but that sub-case is
/// intentionally not generated — see the module docs — so it is not in the
/// vocabulary.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Down,
    Up,
    Space,
    Enter,
    Right,
}

impl Key {
    /// The tmux `send-keys` key name for this keystroke.
    pub fn tmux_name(self) -> &'static str {
        match self {
            Key::Down => "Down",
            Key::Up => "Up",
            Key::Space => "Space",
            Key::Enter => "Enter",
            Key::Right => "Right",
        }
    }
}

/// The shape of one question, as the generator needs it: its option count and
/// whether more than one option may be selected. Parsed from the stored
/// `{"questions":[…]}` tool input (see [`parse_question_shapes`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestionShape {
    /// How many real options the question lists (the navigable range).
    pub option_count: usize,
    /// Whether the question is multi-select (`[ ]` checkboxes) vs single-select.
    pub multi_select: bool,
}

/// Why a key sequence could not be generated for the requested answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionKeyError {
    /// No questions were provided, so there is nothing to answer.
    NoQuestions,
    /// The number of selection groups did not match the number of questions.
    SelectionCountMismatch { questions: usize, selections: usize },
    /// A selected option index is outside the question's option range.
    OptionOutOfRange {
        question: usize,
        option: usize,
        option_count: usize,
    },
    /// A single-select question was given other than exactly one selection.
    SingleSelectNeedsOneOption { question: usize, selected: usize },
    /// A multi-select question was given no selections (nothing to submit).
    MultiSelectNeedsSelection { question: usize },
    /// Multi-select inside a multi-question call is intentionally unsupported:
    /// its tab-advance behavior was not part of the verified TUI probe, so a
    /// generated sequence could mis-answer. Answer such a call in the terminal.
    MultiSelectInMultiQuestion { question: usize },
}

/// Build the exact keystrokes that answer an `AskUserQuestion` prompt, given
/// each question's shape and the option index(es) selected for it.
///
/// `selections[q]` holds the 0-based option indices chosen for `questions[q]`:
/// exactly one for a single-select question, one or more for a multi-select
/// one. The returned [`Key`]s are sent to the live TUI in order (see the module
/// docs for the pinned per-shape sequences). Returns a [`QuestionKeyError`]
/// when the request is malformed or names a sub-case the generator refuses to
/// guess at (multi-select within a multi-question call).
pub fn answer_keys(
    questions: &[QuestionShape],
    selections: &[Vec<usize>],
) -> Result<Vec<Key>, QuestionKeyError> {
    if questions.is_empty() {
        return Err(QuestionKeyError::NoQuestions);
    }
    if questions.len() != selections.len() {
        return Err(QuestionKeyError::SelectionCountMismatch {
            questions: questions.len(),
            selections: selections.len(),
        });
    }

    let multi_question = questions.len() > 1;
    let mut keys = Vec::new();

    for (qi, (question, selected)) in questions.iter().zip(selections).enumerate() {
        for &option in selected {
            if option >= question.option_count {
                return Err(QuestionKeyError::OptionOutOfRange {
                    question: qi,
                    option,
                    option_count: question.option_count,
                });
            }
        }

        if question.multi_select {
            if multi_question {
                // The tab-advance for a toggled multi-select question was not
                // part of the verified probe; refuse rather than guess.
                return Err(QuestionKeyError::MultiSelectInMultiQuestion { question: qi });
            }
            if selected.is_empty() {
                return Err(QuestionKeyError::MultiSelectNeedsSelection { question: qi });
            }
            // Toggle each selected option: move the highlight to it (tracking
            // the cursor so the move is the signed delta) and press Space.
            let mut cursor = 0usize;
            for &option in selected {
                push_move(&mut keys, cursor, option);
                keys.push(Key::Space);
                cursor = option;
            }
            // Right moves to the Submit tab; Enter submits.
            keys.push(Key::Right);
            keys.push(Key::Enter);
        } else {
            if selected.len() != 1 {
                return Err(QuestionKeyError::SingleSelectNeedsOneOption {
                    question: qi,
                    selected: selected.len(),
                });
            }
            // The highlight starts on option 0, so Down × index reaches the
            // chosen option; Enter records it (and auto-advances to the next
            // question's tab in a multi-question call).
            for _ in 0..selected[0] {
                keys.push(Key::Down);
            }
            keys.push(Key::Enter);
        }
    }

    // A multi-question call lands on a "Review your answers" screen after the
    // last question records (default on "Submit answers"); one Enter submits.
    if multi_question {
        keys.push(Key::Enter);
    }

    Ok(keys)
}

/// Push the `Down`/`Up` keystrokes that move the highlight from `from` to `to`.
fn push_move(keys: &mut Vec<Key>, from: usize, to: usize) {
    if to > from {
        for _ in from..to {
            keys.push(Key::Down);
        }
    } else {
        for _ in to..from {
            keys.push(Key::Up);
        }
    }
}

/// Parse the question shapes (option count + `multiSelect`) the generator needs
/// from the stored `{"questions":[…]}` tool input.
///
/// Defensive by design: the JSON comes straight off the wire. A payload that is
/// unparsable, is not the expected object/array shape, or lists no questions
/// yields `None`, so the caller treats it as "cannot answer from the UI" and
/// falls back to the terminal rather than driving the TUI from a guess.
pub fn parse_question_shapes(tool_input_json: &str) -> Option<Vec<QuestionShape>> {
    let parsed: serde_json::Value = serde_json::from_str(tool_input_json).ok()?;
    let questions = parsed.get("questions")?.as_array()?;
    if questions.is_empty() {
        return None;
    }
    let shapes = questions
        .iter()
        .map(|question| {
            let option_count = question
                .get("options")
                .and_then(|options| options.as_array())
                .map_or(0, |options| options.len());
            let multi_select = question
                .get("multiSelect")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            QuestionShape {
                option_count,
                multi_select,
            }
        })
        .collect();
    Some(shapes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single(option_count: usize) -> QuestionShape {
        QuestionShape {
            option_count,
            multi_select: false,
        }
    }

    fn multi(option_count: usize) -> QuestionShape {
        QuestionShape {
            option_count,
            multi_select: true,
        }
    }

    #[test]
    fn single_question_single_select_picks_the_first_option_with_a_lone_enter() {
        // Option 0 is already highlighted, so no movement — just submit.
        assert_eq!(
            answer_keys(&[single(3)], &[vec![0]]).unwrap(),
            vec![Key::Enter],
        );
    }

    #[test]
    fn single_question_single_select_steps_down_to_the_chosen_option() {
        // Pick option 2 (0-based): Down twice to reach it, then Enter submits.
        assert_eq!(
            answer_keys(&[single(4)], &[vec![2]]).unwrap(),
            vec![Key::Down, Key::Down, Key::Enter],
        );
    }

    #[test]
    fn single_question_single_select_ignores_trailing_non_answer_rows() {
        // option_count bounds the navigation: picking the last real option of a
        // 2-option question is a single Down + Enter, never reaching the
        // "Type something"/"Chat about this" rows that follow.
        assert_eq!(
            answer_keys(&[single(2)], &[vec![1]]).unwrap(),
            vec![Key::Down, Key::Enter],
        );
    }

    #[test]
    fn single_question_multi_select_toggles_each_selection_then_submits() {
        // Toggle options 0 and 2: Space at 0, Down×2 to 2, Space, then Right to
        // the Submit tab and Enter. Enter is never used to toggle here.
        assert_eq!(
            answer_keys(&[multi(3)], &[vec![0, 2]]).unwrap(),
            vec![
                Key::Space,
                Key::Down,
                Key::Down,
                Key::Space,
                Key::Right,
                Key::Enter,
            ],
        );
    }

    #[test]
    fn single_question_multi_select_moves_up_when_a_later_selection_precedes() {
        // Selections out of ascending order move the highlight back up by the
        // delta: toggle 2 (Down×2, Space), then 1 (Up once, Space), submit.
        assert_eq!(
            answer_keys(&[multi(3)], &[vec![2, 1]]).unwrap(),
            vec![
                Key::Down,
                Key::Down,
                Key::Space,
                Key::Up,
                Key::Space,
                Key::Right,
                Key::Enter,
            ],
        );
    }

    #[test]
    fn multi_question_single_select_records_each_then_submits_the_review() {
        // Q1 pick option 1 (Down, Enter auto-advances), Q2 pick option 0
        // (Enter auto-advances to the review), then one Enter submits the
        // "Review your answers" screen.
        assert_eq!(
            answer_keys(&[single(2), single(3)], &[vec![1], vec![0]]).unwrap(),
            vec![Key::Down, Key::Enter, Key::Enter, Key::Enter],
        );
    }

    #[test]
    fn no_questions_is_an_error() {
        assert_eq!(answer_keys(&[], &[]), Err(QuestionKeyError::NoQuestions));
    }

    #[test]
    fn selection_count_must_match_question_count() {
        assert_eq!(
            answer_keys(&[single(2), single(2)], &[vec![0]]),
            Err(QuestionKeyError::SelectionCountMismatch {
                questions: 2,
                selections: 1,
            }),
        );
    }

    #[test]
    fn an_out_of_range_option_is_rejected() {
        assert_eq!(
            answer_keys(&[single(2)], &[vec![5]]),
            Err(QuestionKeyError::OptionOutOfRange {
                question: 0,
                option: 5,
                option_count: 2,
            }),
        );
    }

    #[test]
    fn single_select_needs_exactly_one_option() {
        assert_eq!(
            answer_keys(&[single(3)], &[vec![0, 1]]),
            Err(QuestionKeyError::SingleSelectNeedsOneOption {
                question: 0,
                selected: 2,
            }),
        );
        assert_eq!(
            answer_keys(&[single(3)], &[vec![]]),
            Err(QuestionKeyError::SingleSelectNeedsOneOption {
                question: 0,
                selected: 0,
            }),
        );
    }

    #[test]
    fn multi_select_needs_at_least_one_selection() {
        assert_eq!(
            answer_keys(&[multi(3)], &[vec![]]),
            Err(QuestionKeyError::MultiSelectNeedsSelection { question: 0 }),
        );
    }

    #[test]
    fn multi_select_within_a_multi_question_call_is_refused() {
        // The tab-advance for a toggled multi-select question was not verified,
        // so the generator refuses rather than guess; the user answers in the
        // terminal instead.
        assert_eq!(
            answer_keys(&[single(2), multi(2)], &[vec![0], vec![1]]),
            Err(QuestionKeyError::MultiSelectInMultiQuestion { question: 1 }),
        );
    }

    #[test]
    fn tmux_names_match_the_send_keys_vocabulary() {
        assert_eq!(Key::Down.tmux_name(), "Down");
        assert_eq!(Key::Up.tmux_name(), "Up");
        assert_eq!(Key::Space.tmux_name(), "Space");
        assert_eq!(Key::Enter.tmux_name(), "Enter");
        assert_eq!(Key::Right.tmux_name(), "Right");
    }

    #[test]
    fn parse_extracts_option_count_and_multi_select_flag() {
        let json = r#"{
            "questions": [
                { "header": "A", "options": [{"label":"x"},{"label":"y"}], "multiSelect": true },
                { "header": "B", "options": [{"label":"z"}] }
            ]
        }"#;
        assert_eq!(
            parse_question_shapes(json),
            Some(vec![multi(2), single(1)]),
        );
    }

    #[test]
    fn parse_rejects_unparsable_or_empty_payloads() {
        assert_eq!(parse_question_shapes("not json"), None);
        assert_eq!(parse_question_shapes("{}"), None);
        assert_eq!(parse_question_shapes(r#"{"questions":[]}"#), None);
    }
}
