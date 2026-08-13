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

/// Path, relative to this crate's manifest directory, of the combined v2 schema
/// document — the client-request + notification surface Delta pins. v1 is a
/// legacy `initialize`-only stub and is not vendored (see the vendor directory's
/// `README.md`).
pub const V2_COMBINED_SCHEMA_RELATIVE_PATH: &str =
    "vendor/app-server-schema/codex_app_server_protocol.v2.schemas.json";

/// Path, relative to this crate's manifest directory, of the combined
/// **non-versioned** schema document. Unlike the v2 combined document, this one
/// carries the `ServerRequest` registry — the server → client request surface,
/// including the `*RequestApproval` approval methods empirically confirmed to
/// drive `turn/start` turns. It is the ground-truth reference the approval
/// fan-out in [`crate::translate`] is reconciled against (see the vendor
/// directory's `README.md` for why v2 alone is insufficient).
pub const COMBINED_SERVER_REQUEST_SCHEMA_RELATIVE_PATH: &str =
    "vendor/app-server-schema/codex_app_server_protocol.schemas.json";

/// Directory, relative to this crate's manifest directory, holding the vendored
/// v2 schema split one file per type (`ThreadStartParams.json`, …). Same
/// provenance as the combined v2 document; convenient when a single request
/// shape is the thing being reconciled against.
pub const V2_PER_TYPE_SCHEMA_RELATIVE_DIR: &str = "vendor/app-server-schema/v2";

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

    /// The combined non-versioned schema is present and carries the
    /// `ServerRequest` registry with the three approval methods — the artifact
    /// PR #267 omitted. This is the proof the server → client request surface is
    /// now vendored, so the approval fan-out has a ground-truth reference.
    #[test]
    fn vendored_server_request_schema_carries_the_approval_registry() {
        let path = format!(
            "{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            COMBINED_SERVER_REQUEST_SCHEMA_RELATIVE_PATH
        );
        let raw = std::fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!("vendored combined server-request schema missing at {path}: {err}")
        });
        let doc: serde_json::Value = serde_json::from_str(&raw)
            .expect("vendored combined server-request schema is not valid JSON");

        // The `ServerRequest` type is the server → client request registry the
        // v2 combined document omits; its presence is the cheapest proof the gap
        // is closed.
        assert!(
            doc.pointer("/definitions/ServerRequest").is_some(),
            "vendored combined schema has no `ServerRequest` definition"
        );

        // Every approval method Delta reconciles against must appear in the
        // vendored registry, so the fan-out cannot silently drift from ground
        // truth.
        for method in [
            "item/commandExecution/requestApproval",
            "item/fileChange/requestApproval",
            "item/permissions/requestApproval",
        ] {
            assert!(
                raw.contains(method),
                "vendored server-request schema is missing approval method `{method}`"
            );
        }
    }

    /// Read one vendored per-type v2 schema document.
    fn per_type_schema(type_name: &str) -> serde_json::Value {
        let path = format!(
            "{}/{}/{type_name}.json",
            env!("CARGO_MANIFEST_DIR"),
            V2_PER_TYPE_SCHEMA_RELATIVE_DIR
        );
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("vendored schema missing at {path}: {err}"));
        serde_json::from_str(&raw)
            .unwrap_or_else(|err| panic!("vendored schema at {path} is not valid JSON: {err}"))
    }

    /// The two schema facts the launch-option pass-through rests on.
    ///
    /// 1. `cwd` is a real `ThreadStartParams` field, so guarding it as
    ///    Delta-owned guards something that exists; and `ThreadStartParams`
    ///    marks nothing required, so a launch carrying only Delta's own fields
    ///    is a complete request.
    /// 2. `ThreadResumeParams` requires **only** `threadId`: its config fields
    ///    (`model`, `sandbox`, `approvalPolicy`, `config`, …) are optional
    ///    overrides of what the resumed thread already carries. That is the
    ///    basis for not replaying a session's launch options on resume — a
    ///    resume that names none keeps the thread exactly as `thread/start`
    ///    configured it. If a future Codex made any of them required, this
    ///    fails and the decision gets revisited.
    #[test]
    fn thread_start_and_resume_params_leave_config_fields_optional() {
        let start = per_type_schema("ThreadStartParams");
        assert!(
            start.pointer("/properties/cwd").is_some(),
            "`cwd` must be a real ThreadStartParams field for the Delta-owned guard to mean anything"
        );
        assert!(
            start.get("required").is_none(),
            "ThreadStartParams is expected to require no field, got {:?}",
            start.get("required")
        );

        let resume = per_type_schema("ThreadResumeParams");
        assert_eq!(
            resume.get("required"),
            Some(&serde_json::json!(["threadId"])),
            "thread/resume must require only the thread id, leaving every config \
             field an optional override"
        );
    }

    /// The schema half of the worktree git-directory grant: `config` is a
    /// free-form object on **both** `thread/start` and `thread/resume`, so the
    /// grant has a documented field to ride on either path.
    ///
    /// That is all the schema can say. It declares no key names, so it cannot
    /// tell anyone whether a **dotted** key
    /// (`sandbox_workspace_write.writable_roots`) is honoured at the leaf the
    /// way the CLI's `-c` flag is — only the real server can, which is why the
    /// `real_thread_start_honors_the_worktree_git_grant` canary exists. Keeping
    /// both is deliberate: this test fails if the field the grant rides on ever
    /// stops being a free-form object, the canary fails if the real server stops
    /// honouring the spelling Delta sends.
    #[test]
    fn thread_start_and_resume_carry_a_free_form_config_object() {
        for type_name in ["ThreadStartParams", "ThreadResumeParams"] {
            let schema = per_type_schema(type_name);
            let config = schema
                .pointer("/properties/config")
                .unwrap_or_else(|| panic!("{type_name} must declare a `config` field"));
            assert_eq!(
                config.get("type"),
                Some(&serde_json::json!(["object", "null"])),
                "{type_name}.config must be a nullable object, got {config}"
            );
            assert_eq!(
                config.get("additionalProperties"),
                Some(&serde_json::json!(true)),
                "{type_name}.config must accept arbitrary keys for the grant to \
                 ride on, got {config}"
            );
        }
    }

    #[test]
    fn vendored_codex_version_is_pinned() {
        assert!(
            !VENDORED_CODEX_VERSION.is_empty(),
            "the vendored Codex version must be pinned"
        );
    }
}
