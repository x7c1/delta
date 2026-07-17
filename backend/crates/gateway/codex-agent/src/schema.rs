//! Pin + accessors for the vendored `codex app-server` protocol schema.
//!
//! The schema itself lives under `vendor/app-server-schema/` (see that
//! directory's `README.md` for provenance and the v1-vs-v2 distinction). This
//! module records the exact Codex version it was generated from and points at
//! the authoritative combined v2 document, so future drift detection has a
//! single programmatic baseline.

/// The Codex CLI version the vendored schema was generated from.
///
/// Drift detection regenerates the schema at this version and compares it
/// against the vendored copy; when re-vendoring against a newer Codex, bump this
/// and replace the files under `vendor/app-server-schema/` in the same change.
pub const VENDORED_CODEX_VERSION: &str = "0.144.4";

/// Path, relative to this crate's manifest directory, of the authoritative
/// combined v2 schema document — the single file reconciliation validates
/// against. Delta pins **v2**; v1 is a legacy `initialize`-only stub and is not
/// vendored (see the vendor directory's `README.md`).
pub const V2_COMBINED_SCHEMA_RELATIVE_PATH: &str =
    "vendor/app-server-schema/codex_app_server_protocol.v2.schemas.json";

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test for the vendored artifact: the combined v2 schema is present,
    /// is well-formed JSON, and carries the expected top-level shape.
    ///
    /// NOTE: this is deliberately only a presence/well-formedness check. A
    /// strict "Delta's wire types match this schema" conformance/drift test is
    /// **not** written here yet: Delta's current Codex wire types are known to
    /// diverge from the real protocol, so such a test would fail today. It
    /// lands with the wire-reconciliation slices, once the types are aligned.
    #[test]
    fn vendored_v2_schema_is_present_and_well_formed() {
        let path = format!(
            "{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            V2_COMBINED_SCHEMA_RELATIVE_PATH
        );
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("vendored v2 schema missing at {path}: {err}"));
        assert!(!raw.trim().is_empty(), "vendored v2 schema is empty");

        let doc: serde_json::Value =
            serde_json::from_str(&raw).expect("vendored v2 schema is not valid JSON");

        // The generator emits a JSON Schema with a `definitions` map; a
        // non-empty map is the cheapest proof the dump is the real protocol
        // rather than a stub.
        let definitions = doc
            .get("definitions")
            .and_then(serde_json::Value::as_object)
            .expect("vendored v2 schema has no `definitions` object");
        assert!(
            !definitions.is_empty(),
            "vendored v2 schema has an empty `definitions` map"
        );
    }

    #[test]
    fn vendored_codex_version_is_pinned() {
        assert!(
            !VENDORED_CODEX_VERSION.is_empty(),
            "the vendored Codex version must be pinned"
        );
    }
}
