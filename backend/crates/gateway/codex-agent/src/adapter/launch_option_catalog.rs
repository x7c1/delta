//! What Delta ships for Codex: the declared launch-option catalog and the
//! guards that keep it shippable.
//!
//! Its own module inside the adapter because it grows one entry at a time —
//! every addition brings a value to check against the vendored schema — while
//! the rest of the adapter is about driving a live session.

use delta_usecase::{AgentProvider, LaunchOptionPreset};

use super::CONFIG_FIELD;

/// The launch options Delta ships for Codex.
///
/// Declared in the adapter that owns Codex, beside [`CODEX_CAPABILITIES`] —
/// whose `launch_option_style` is what says a Codex option's `name` is a
/// `thread/start` field — for the same reason the capability profile lives
/// there: the adapter that owns the behaviour owns the vocabulary. The
/// composition root reads this through one per-provider accessor and
/// materializes every entry into the launch-option registry at startup, so each
/// is an ordinary registry row the user can tick.
///
/// Field names are the wire spelling, exactly as [`thread_start_params`] emits
/// them: `approvalsReviewer`, not `approvals_reviewer`. Shipping them is most of
/// the point — a hand-typed snake_case field fails at the codex server with
/// nothing Delta can say about it.
///
/// Deliberately short. Codex's `model` is **not** shipped: unlike Claude it has
/// no aliases, so any entry would be a dated snapshot of a concrete slug.
/// `sandbox` and `personality` are not shipped either — not in use.
///
/// The `config` entry is a **starting point to copy**, not something most users
/// select as-is: `config` is a single `thread/start` field and
/// [`thread_start_params`] rejects the same field twice, so this row and a
/// user's own `config` row are mutually exclusive. That is the intended flow —
/// real `config` values carry machine-specific paths
/// (`sandbox_workspace_write.writable_roots`), so the user duplicates this row,
/// adds their paths and selects theirs. Shipping it means they do not have to
/// discover the JSON key names first.
///
/// Adapting it has one consequence worth knowing: a selected `config` that
/// states anything at or under `sandbox_workspace_write` is what makes Delta
/// stand aside from the worktree git grant (`apply_worktree_git_grant`), so a
/// session in a Delta-created worktree gets no `<repo-root>/.git` grant of its
/// own and git writes inside it can raise approval prompts. A copy that lists
/// its own `writable_roots` therefore has to include that path itself. The
/// shipped value says nothing about the sandbox, so selecting it as-is leaves
/// the grant in place.
///
/// Guard tests below pin that no entry names a `DELTA_OWNED_THREAD_FIELDS`
/// field, that every value backed by a schema enum is still a member of it in
/// the vendored schema, and that the `config` value parses as JSON (a
/// non-parsing value is passed through as a bare string by
/// `thread_start_value`, which would be silently inert).
///
/// [`CODEX_CAPABILITIES`]: super::CODEX_CAPABILITIES
/// [`thread_start_params`]: super::thread_start_params
pub const CODEX_LAUNCH_OPTION_CATALOG: &[LaunchOptionPreset] = &[
    LaunchOptionPreset {
        key: "codex:approvals-reviewer-auto-review",
        label: "Auto review approvals",
        name: "approvalsReviewer",
        value: Some("auto_review"),
        provider: AgentProvider::Codex,
    },
    LaunchOptionPreset {
        key: "codex:approval-policy-on-request",
        label: "Approvals: on request",
        name: "approvalPolicy",
        value: Some("on-request"),
        provider: AgentProvider::Codex,
    },
    LaunchOptionPreset {
        key: "codex:config-reasoning-summary",
        label: "Config: reasoning summary",
        name: CONFIG_FIELD,
        value: Some(r#"{"model_reasoning_summary": "auto"}"#),
        provider: AgentProvider::Codex,
    },
];

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    use crate::adapter::{thread_start_value, DELTA_OWNED_THREAD_FIELDS};
    use crate::schema::V2_COMBINED_SCHEMA_RELATIVE_PATH;

    /// The vendored combined v2 schema, parsed.
    fn vendored_v2_schema() -> Value {
        let path = format!(
            "{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            V2_COMBINED_SCHEMA_RELATIVE_PATH
        );
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("vendored v2 schema missing at {path}: {err}"));
        serde_json::from_str(&raw).expect("vendored v2 schema is not valid JSON")
    }

    /// Collect the string members of a schema definition's enum, following the
    /// one `oneOf` branch shape the app-server generator emits for an enum that
    /// also has a structured form (`AskForApproval`'s `granular` object).
    fn string_enum_members(schema: &Value, definition: &str) -> Vec<String> {
        let node = schema
            .pointer(&format!("/definitions/{definition}"))
            .unwrap_or_else(|| panic!("vendored v2 schema has no `{definition}` definition"));
        let members = |node: &Value| -> Vec<String> {
            node.get("enum")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut out = members(node);
        if let Some(branches) = node.get("oneOf").and_then(Value::as_array) {
            for branch in branches {
                out.extend(members(branch));
            }
        }
        assert!(
            !out.is_empty(),
            "`{definition}` declares no string enum members in the vendored schema"
        );
        out
    }

    /// One shipped option, by the `name` it sets.
    fn shipped(name: &str) -> &'static LaunchOptionPreset {
        CODEX_LAUNCH_OPTION_CATALOG
            .iter()
            .find(|preset| preset.name == name)
            .unwrap_or_else(|| panic!("no built-in Codex option sets `{name}`"))
    }

    /// No shipped option may name a field Delta fills in itself.
    ///
    /// The launch path rejects such an option at runtime, so this would surface
    /// as a failed spawn rather than a silent override — but a shipped option
    /// that can only ever fail is a shipped bug, and this catches it in CI
    /// instead of on the user's first click.
    #[test]
    fn no_shipped_option_names_a_delta_owned_field() {
        for preset in CODEX_LAUNCH_OPTION_CATALOG {
            assert!(
                !DELTA_OWNED_THREAD_FIELDS.contains(&preset.name),
                "built-in `{}` names `{}`, which Delta sets itself",
                preset.key,
                preset.name
            );
        }
    }

    /// Every shipped value that has an enum behind it is still a member of that
    /// enum in the vendored schema.
    ///
    /// Delta passes a launch option's value through unvalidated, so a value
    /// upstream has retired would reach the codex server and be refused there,
    /// with nothing Delta can say about it. Reading the enums back from the
    /// vendored schema means re-vendoring against a newer Codex is what
    /// surfaces the retirement.
    #[test]
    fn shipped_enum_values_are_members_of_the_vendored_schema_enums() {
        let schema = vendored_v2_schema();
        for (field, definition) in [
            ("approvalsReviewer", "ApprovalsReviewer"),
            ("approvalPolicy", "AskForApproval"),
        ] {
            let preset = shipped(field);
            let value = preset
                .value
                .unwrap_or_else(|| panic!("built-in `{}` must carry a value", preset.key));
            let members = string_enum_members(&schema, definition);
            assert!(
                members.iter().any(|member| member == value),
                "built-in `{}` sets {field} = `{value}`, which is not a member of \
                 `{definition}` in the vendored schema ({members:?})",
                preset.key
            );
        }
    }

    /// The shipped `config` value parses as JSON.
    ///
    /// `thread_start_value` falls back to passing a non-JSON value through as
    /// a bare string, so a typo in this object would reach `thread/start` as a
    /// string where an object is expected — silently inert rather than an error.
    /// It is also the value the user is meant to copy and adapt, so it has to be
    /// a valid starting point.
    #[test]
    fn the_shipped_config_value_parses_as_a_json_object() {
        let preset = shipped(CONFIG_FIELD);
        let value = preset
            .value
            .unwrap_or_else(|| panic!("built-in `{}` must carry a value", preset.key));
        let parsed: Value = serde_json::from_str(value)
            .unwrap_or_else(|err| panic!("built-in `{}` value is not JSON: {err}", preset.key));
        assert!(
            parsed.is_object(),
            "built-in `{}` value must be a JSON object, got {parsed}",
            preset.key
        );
        // And the same fallback must not silently rewrite it on the way out.
        assert_eq!(
            thread_start_value(Some(value)),
            parsed,
            "the launch path must send the shipped `config` value as the object it parses to"
        );
    }
}
