//! Transcript-sync use-case tests.
//!
//! These exercise the I/O shell around the pure attribution fold: cursor
//! handling, seed reads, effect execution, event broadcast, and the hook /
//! poll entry points. The attribution *rules* themselves (which thread a
//! line lands on) are pinned by the pure `delta-attribution` test suite;
//! purely-attributional cases live there, not here.

mod support;

mod branch_send_attributes_late_arriving_lines_to_child;
mod branch_send_attributes_user_and_assistant_to_child;
mod db_behind_mis_seeds_carry_thread_to_main_for_a_leading_non_user_line;
mod db_behind_transcript_reports_no_latest_user_thread;
mod ingesting_tool_result_resolves_the_correlated_permission;
mod interrupt_marker_emits_turn_interrupted_and_stays_on_thread;
mod open_session_seeds_carry_thread_from_branch_so_leading_line_is_not_main;
mod open_session_syncs_existing_transcript_so_latest_user_thread_is_known;
mod plain_send_attributes_user_and_assistant_to_main;
mod poll_transcript_groups_new_lines_per_session;
mod poll_transcript_ingests_assistant_line_flushed_after_stop;
mod poll_transcript_only_polls_open_sessions;
mod poll_transcript_without_session_is_empty;
mod queued_command_send_attributes_user_and_assistant_to_child;
mod skipped_line_does_not_stall_later_turn_ingestion;
