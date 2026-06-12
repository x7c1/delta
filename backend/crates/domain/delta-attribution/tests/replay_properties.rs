//! Replay properties over the golden corpus.
//!
//! Two properties hold for every corpus case:
//!
//! 1. **Determinism** — replaying the same inputs twice yields identical
//!    output (the fold reads nothing but its arguments).
//! 2. **Batch-split invariance** — folding the lines split across *arbitrary*
//!    batch boundaries, threading the returned state into the next batch,
//!    yields exactly the messages, effects, and final state of the one-batch
//!    fold. The live ingestion cursor can cut a transcript anywhere (per
//!    hook, per poll tick, across restarts), so this is the property that
//!    proves cursor resumption can never change attribution.

mod support;

use delta_attribution::{attribute_lines, Attributed, TranscriptMessage};
use support::corpus::{load_cases, CorpusCase};

#[test]
fn replaying_a_case_twice_is_deterministic() {
    for case in load_cases() {
        assert_eq!(
            case.replay(),
            case.replay(),
            "case {}: two replays of the same inputs diverged",
            case.name
        );
    }
}

#[test]
fn every_two_way_batch_split_matches_the_one_batch_fold() {
    for case in load_cases() {
        let whole = case.replay();
        for cut in 0..=case.lines.len() {
            let split = fold_in_batches(&case, &[cut]);
            assert_eq!(
                whole, split,
                "case {}: splitting the batch at line {cut} changed the outcome",
                case.name
            );
        }
    }
}

#[test]
fn one_line_per_batch_matches_the_one_batch_fold() {
    for case in load_cases() {
        let whole = case.replay();
        let cuts: Vec<usize> = (1..case.lines.len()).collect();
        assert_eq!(
            whole,
            fold_in_batches(&case, &cuts),
            "case {}: per-line batching changed the outcome",
            case.name
        );
    }
}

#[test]
fn random_multi_way_batch_splits_match_the_one_batch_fold() {
    for case in load_cases() {
        let whole = case.replay();
        // A small deterministic xorshift stream seeds the cut points, so the
        // test explores irregular batchings without flaking.
        let mut rng: u64 = 0x5DEECE66D;
        for round in 0..16 {
            let mut cuts: Vec<usize> = (0..(case.lines.len() / 2).max(1))
                .map(|_| {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    (rng as usize) % (case.lines.len() + 1)
                })
                .collect();
            cuts.sort_unstable();
            cuts.dedup();
            assert_eq!(
                whole,
                fold_in_batches(&case, &cuts),
                "case {}: random batching (round {round}, cuts {cuts:?}) changed the outcome",
                case.name
            );
        }
    }
}

/// Fold a case's lines as consecutive batches separated at `cuts` (sorted,
/// deduped 0-based line offsets), threading the state between batches the way
/// the actor threads it through the store between syncs, and stitch the
/// per-batch outputs back together.
fn fold_in_batches(case: &CorpusCase, cuts: &[usize]) -> Attributed {
    let session = case.session();
    let mut state = case.replay_seed();
    let mut messages = Vec::new();
    let mut effects = Vec::new();

    let mut start = 0;
    let bounds = cuts.iter().copied().chain([case.lines.len()]);
    for end in bounds {
        let batch: Vec<TranscriptMessage> = case.lines[start..end].to_vec();
        let outcome = attribute_lines(&session, case.main_thread, state, batch);
        messages.extend(outcome.messages);
        effects.extend(outcome.effects);
        state = outcome.state;
        start = end;
    }

    Attributed {
        messages,
        effects,
        state,
    }
}
