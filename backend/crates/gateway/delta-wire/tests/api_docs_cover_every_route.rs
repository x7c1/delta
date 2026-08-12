//! Every declared route is described in the prose API reference.
//!
//! `ENDPOINTS` is the machine-readable inventory of the API surface, and
//! `docs/guides/api/` is the prose one. The two are written by hand in
//! different places, so without a check the prose silently falls behind: a PR
//! adds an endpoint, the router mounts it, and the only document a reader
//! consults never mentions it.
//!
//! This test walks the real table — not a copy — and asserts that each route,
//! spelled the one way [`route_label`] spells it, appears in at least one
//! markdown file of that directory. Declaring a new endpoint therefore fails
//! `cargo test` until it is written up.
//!
//! It checks presence, not accuracy: prose that names a route while describing
//! it wrongly is beyond what any mechanical check can catch.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use delta_wire::endpoint::{route_label, ENDPOINTS};

/// The prose API reference, resolved relative to this crate rather than to the
/// process's working directory (which `cargo test` does not pin).
fn docs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../docs/guides/api")
}

/// Every markdown file in `dir`, concatenated, so one route may be documented
/// in whichever file its area lives in.
fn markdown_in(dir: &Path) -> String {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|err| {
        panic!(
            "cannot read the API docs directory {}: {err}",
            dir.display()
        )
    });

    let mut paths: Vec<PathBuf> = entries
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    // Sorted so a failure is reported against a stable reading of the directory.
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no markdown found in {} — the docs moved, and this check is now vacuous",
        dir.display(),
    );

    paths
        .iter()
        .map(|path| {
            std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `docs` names `label` as a route of its own.
///
/// A plain substring test would let `GET /api/sessions/{id}/threads` stand in
/// for `GET /api/sessions`, since the shorter label is a prefix of the longer
/// one. Requiring that the next character not continue the path closes that:
/// another segment always starts with `/`, while the trailing backtick of a
/// heading, a query string, or end-of-file all end the route.
fn documents(docs: &str, label: &str) -> bool {
    docs.match_indices(label)
        .any(|(at, _)| !docs[at + label.len()..].starts_with('/'))
}

#[test]
fn every_declared_route_is_documented() {
    let dir = docs_dir();
    let docs = markdown_in(&dir);

    let undocumented: BTreeSet<String> = ENDPOINTS
        .iter()
        .map(|spec| route_label(spec.method, spec.path))
        .filter(|label| !documents(&docs, label))
        .collect();

    assert!(
        undocumented.is_empty(),
        "declared in delta_wire::endpoint::ENDPOINTS but not documented in {}: {}\n\
         Add a section naming each route exactly as spelled here (e.g. ``### `POST /api/sends` ``).",
        dir.display(),
        undocumented.into_iter().collect::<Vec<_>>().join(", "),
    );
}

#[test]
fn a_route_the_docs_never_name_is_reported() {
    // Pins the check itself: without this, a `documents` that answered `true`
    // for everything would leave the test above passing while catching nothing.
    let docs = markdown_in(&docs_dir());
    assert!(!documents(&docs, "GET /api/undocumented"));
}

#[test]
fn a_longer_route_does_not_document_its_prefix() {
    // The prefix case the substring test would get wrong.
    let docs = "### `GET /api/sessions/{id}/threads`";
    assert!(documents(docs, "GET /api/sessions/{id}/threads"));
    assert!(!documents(docs, "GET /api/sessions"));
}
