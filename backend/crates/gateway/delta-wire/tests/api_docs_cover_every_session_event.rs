//! Every `/ws` session event is described in the prose API reference.
//!
//! The sibling check in `api_docs_cover_every_route.rs` works at route
//! granularity, which cannot see this gap: `/ws` is a single route whose body
//! is a tagged union, so an event may be added — and shipped to the browser —
//! while the one document a client author reads still lists the old set. That
//! is exactly what happened: the section described 8 of 18 events while
//! claiming it could not drift.
//!
//! This test walks [`event_kinds`], which serializes one sample of every
//! variant and is therefore the same list serde puts on the wire, and asserts
//! each `kind` appears in the `/ws` page as JSON — `"kind": "<name>"`, the form
//! an example carries. Adding a variant therefore fails `cargo test` until it
//! is written up.
//!
//! It checks presence, not accuracy: prose that names an event while describing
//! its payload wrongly is beyond what any mechanical check can catch.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use delta_wire::event_kinds;

/// The page documenting the live channels, resolved relative to this crate
/// rather than to the process's working directory (which `cargo test` does not
/// pin).
fn doc_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../docs/guides/api/live-channels.md")
}

fn read_doc() -> String {
    let path = doc_path();
    std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "cannot read the live-channels page {}: {err}",
            path.display()
        )
    })
}

/// Whether `docs` shows `kind` the way a frame carries it.
///
/// Matching the JSON form rather than the bare name is what makes the check
/// mean "there is an example": every bullet in that section opens with its
/// event's name, so a substring test on the name alone would be satisfied by
/// prose that shows no shape at all.
fn documents_kind(docs: &str, kind: &str) -> bool {
    docs.contains(&format!("\"kind\": \"{kind}\""))
}

#[test]
fn every_session_event_kind_is_documented() {
    let docs = read_doc();

    let undocumented: BTreeSet<String> = event_kinds()
        .into_iter()
        .filter(|kind| !documents_kind(&docs, kind))
        .collect();

    assert!(
        undocumented.is_empty(),
        "serialized by delta_wire::WireSessionEvent but not documented in {}: {}\n\
         Add a JSON example (`{{ \"kind\": \"<name>\", … }}`) and an explanatory \
         bullet to the `GET /ws` section for each.",
        doc_path().display(),
        undocumented.into_iter().collect::<Vec<_>>().join(", "),
    );
}

#[test]
fn a_kind_the_docs_never_show_is_reported() {
    // Pins the check itself: without this, a `documents_kind` that answered
    // `true` for everything would leave the test above passing while catching
    // nothing.
    assert!(!documents_kind(&read_doc(), "undocumented_event"));
}

#[test]
fn a_prose_mention_alone_does_not_document_a_kind() {
    // The case the name-only substring test would get wrong.
    let prose = "- `session_registered` — emitted on the first `UserPromptSubmit`.";
    assert!(!documents_kind(prose, "session_registered"));
    assert!(documents_kind(
        "{ \"kind\": \"session_registered\", \"session_id\": \"sess-1\" }",
        "session_registered",
    ));
}
