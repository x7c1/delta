//! Thread-switch context-framing tests.

mod support;

mod revisit_to_branch_injects_switch_note_with_root_quote;
mod revisit_to_main_injects_switch_note_without_quote;
mod same_thread_continuation_injects_nothing;
mod unknown_previous_thread_injects_nothing;
