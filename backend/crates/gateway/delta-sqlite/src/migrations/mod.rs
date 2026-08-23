//! The migration ladder: the single source of truth for Delta's SQLite schema.
//!
//! ## Overview
//!
//! The schema is an ordered list of [`Step`]s, each carrying the
//! `PRAGMA user_version` it produces. Opening a database applies every step
//! above the version the file is stamped with, in ascending order, one
//! transaction per version. A *fresh* database is built by replaying the whole
//! ladder from 0, exactly the way an existing database is upgraded — there is
//! deliberately no second definition of the schema to keep in sync, and
//! therefore nothing that can drift.
//!
//! To read the whole schema at once, dump it from a database the ladder built:
//!
//! ```text
//! sqlite3 delta.db .schema
//! ```
//!
//! ## Layout: grouped by subject, not by version
//!
//! One module per schema subject — [`session`], [`message`], [`send`], … — each
//! owning that subject's entire history: the step that creates it and every
//! later step that alters it, with the design intent behind the shape in the
//! module's own docs. A step declares its own version, so [`session`] may hold
//! steps at v3 and v7 while [`clone_root`] holds one at v5; [`registry`] is what
//! orders them globally. Within one version the registry keeps the declared file
//! order, so an index or trigger step still follows the table it belongs to.
//!
//! ## v3 is a squashed baseline
//!
//! Databases stamped `user_version = 3` already exist, and the ladder was
//! introduced without altering any table. Its first steps are therefore all
//! `to_version: 3` and are the schema as it stood at that moment, split across
//! the per-subject modules. A database already at 3 skips them entirely; a fresh
//! database replays them. Because the baseline is the same statements that
//! created the existing files, both sides land in the same place by
//! construction — which a reconstructed v1/v2/v3 history would not guarantee,
//! since a reconstruction that was even slightly off would make fresh and
//! existing databases diverge silently. Versions 1 and 2 accordingly do not
//! appear on the ladder at all. Every version from 4 onward is a genuine diff.
//!
//! The baseline is therefore a **floor**, not just a starting point: a database
//! stamped 1 or 2 — every overlay written by v0.2.x or v0.3.0 — is refused on
//! open ([`crate::Error::PreBaselineOverlay`]) rather than migrated. Replaying
//! the baseline over it would apply nothing (every baseline statement is
//! `IF NOT EXISTS`, and the tables are already there) and then stamp the file
//! current, leaving, say, v1's narrower `message.role` `CHECK` in place to fail
//! mid-session much later. `make reset` is the honest answer for those.
//!
//! ## Adding a step
//!
//! Append it to its subject's `STEPS` with the next version number, choose
//! [`Step::additive`] or [`Step::destructive`] deliberately (the latter is what
//! makes the runner snapshot the database first), and bump
//! [`crate::SCHEMA_VERSION`] to match. [`validate`] is what catches the
//! forgotten bump: without it, a step whose version exceeds `SCHEMA_VERSION`
//! would simply never be applied to anything.
//!
//! ## Schema-wide conventions
//!
//! Timestamps are ISO-8601 UTC text (SQLite has no native datetime type). Every
//! table is `STRICT` (values must match the declared column types) and value
//! domains are pinned with `CHECK` constraints, so a typo'd status or a mistyped
//! bind surfaces as an immediate error instead of silently persisted garbage.
//! Child tables cascade on session delete, so removing a session row removes
//! everything it owns.
//!
//! The thread overlay — `thread_id`, `semantic_parent_uuid`, threads, the send
//! queue and permission history — is **the irreplaceable data**; message content
//! and the linear parent are a cache rebuildable from the JSONL transcript. That
//! asymmetry is the whole reason this ladder exists rather than a `make reset`
//! prompt: see the compatibility policy doc.

mod clone_root;
mod launch_option;
mod message;
mod permission;
mod prompt_template;
mod runner;
mod send;
mod session;
mod step;
mod subagent;
mod sync_cursor;
mod thread;

#[cfg(test)]
mod tests;

pub(crate) use runner::migrate;
pub(crate) use step::{Step, StepKind};

use crate::error::{Error, Result};

/// The on-disk schema generation this binary expects: the version the ladder's
/// last step produces.
///
/// Reflected into the SQLite file via `PRAGMA user_version` and compared against
/// it on every open ([`crate::SqliteStore::open`]). A database below it is
/// migrated forward by the pending steps; a database *above* it was written by a
/// newer binary and is refused, because the ladder only runs forward.
///
/// Bump this in the same change that appends a step, and never on its own —
/// [`validate`] fails the build's test suite if the two disagree in either
/// direction.
pub const SCHEMA_VERSION: u32 = 5;

/// Every subject's steps, in the order the registry lays them out within a
/// version. Table-creating subjects come before the subjects that reference
/// them, so a replay from empty builds `session` before its children.
const SUBJECTS: &[&[Step]] = &[
    session::STEPS,
    sync_cursor::STEPS,
    thread::STEPS,
    message::STEPS,
    send::STEPS,
    permission::STEPS,
    launch_option::STEPS,
    subagent::STEPS,
    clone_root::STEPS,
    prompt_template::STEPS,
];

/// The whole ladder, flattened and ordered by [`Step::to_version`].
///
/// The sort is **stable**, so steps sharing a version keep [`SUBJECTS`] order —
/// that is the guarantee an index or trigger step relies on to run after the
/// table it belongs to, and the reason a subject can own steps at several
/// versions without having to think about the other subjects.
pub(crate) fn registry() -> Vec<Step> {
    let mut steps: Vec<Step> = SUBJECTS
        .iter()
        .flat_map(|steps| steps.iter().copied())
        .collect();
    steps.sort_by_key(|step| step.to_version);
    steps
}

/// Check that a ladder is internally consistent, before anything is applied.
///
/// - Non-empty, and no step claims to produce version 0 (0 is the "never seen
///   delta" marker a fresh file carries).
/// - Ordered ascending, with **no gap** between consecutive versions: every
///   version from the baseline up to the top is a rung the runner can stand on.
///   The ladder starts at its squashed baseline rather than at 1, so the check
///   is on the distance between neighbours, not on covering `1..=max`.
/// - The highest `to_version` equals `expected_max`, which for the production
///   ladder is [`crate::SCHEMA_VERSION`]. This is the one that catches a step
///   added without bumping the constant: such a step would sit above the target
///   version forever and never be applied to anything, silently.
pub(crate) fn validate(steps: &[Step], expected_max: u32) -> Result<()> {
    let invalid = |reason: String| Err(Error::InvalidLadder(reason));

    let Some(first) = steps.first() else {
        return invalid("the ladder has no steps".to_owned());
    };
    if first.to_version == 0 {
        return invalid("a step must produce a version of at least 1, not 0".to_owned());
    }

    for pair in steps.windows(2) {
        let (previous, next) = (pair[0].to_version, pair[1].to_version);
        if next < previous {
            return invalid(format!(
                "steps are out of order: version {next} follows version {previous}"
            ));
        }
        if next > previous + 1 {
            return invalid(format!(
                "versions {previous} and {next} are not consecutive: \
                 version {} has no steps",
                previous + 1
            ));
        }
    }

    let max = steps[steps.len() - 1].to_version;
    if max != expected_max {
        return invalid(format!(
            "the ladder's highest step produces version {max}, \
             but the expected schema version is {expected_max} \
             (a step was added without bumping it, or the other way round)"
        ));
    }
    Ok(())
}
