//! Golden-corpus replay: every checked-in transcript fixture, fed through the
//! real JSONL parser and the pure attribution fold, must reproduce its
//! checked-in `(thread_id, semantic_parent_uuid)` assignments and effects.
//!
//! See `tests/support/corpus.rs` for the case format and `tests/corpus/cases/`
//! for the inventory. Bless intentional changes with `UPDATE_GOLDEN=1`.

mod support;

use support::corpus::{assert_matches_golden, load_cases};

#[test]
fn every_corpus_case_replays_to_its_golden_assignments() {
    let mut failures = Vec::new();
    for case in load_cases() {
        // Run every case even when an earlier one fails, so one corpus run
        // reports all divergences (and UPDATE_GOLDEN blesses everything).
        if let Err(panic) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| assert_matches_golden(&case)))
        {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "non-string panic".to_owned());
            failures.push(message);
        }
    }
    assert!(
        failures.is_empty(),
        "{} corpus case(s) diverged:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
