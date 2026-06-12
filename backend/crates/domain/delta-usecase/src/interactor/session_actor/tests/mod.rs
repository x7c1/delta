//! Actor-infrastructure tests: cross-session concurrency the lock-era design
//! could not express, per-session mailbox ordering, and actor retirement.

mod a_scripted_input_sequence_executes_in_mailbox_order;
mod an_actor_with_no_runtime_state_retires_after_handling;
mod sessions_ingest_concurrently_while_a_third_handles_a_hook;
