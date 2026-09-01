//! Which Codex launch options switch the codex server's own sandbox or approval
//! gate off.
//!
//! Its own module inside the adapter, beside the declared catalog, because it is
//! Codex vocabulary and nothing above the gateway layer may hold it. The
//! composition root reads it through one per-provider accessor and hands it to
//! the domain as a [`LaunchOptionDangerPolicy`], which is what makes the registry
//! refuse to default-enable such an option and the browser mark it.
//!
//! ## What is reachable
//!
//! A launch option is a `thread/start` field, so the same setting can be said
//! three ways and all three have to be caught:
//!
//! - the dedicated field — `sandbox = danger-full-access`, `approvalPolicy =
//!   never` (or its granular object form with the sandbox/rules gates switched
//!   off);
//! - the free-form `config` object, which carries the same two settings under
//!   their `config.toml` names (`sandbox_mode`, `approval_policy` — the latter
//!   the same `AskForApproval` type as the thread field, granular object form
//!   included) and in either of Codex's two spellings — a nested table or a
//!   dotted key. Both are flattened through the very function the launch's
//!   `config` merge uses ([`super::config_merge::stated_settings`]), so neither
//!   spelling can slip past by being written the other way.
//!
//! Every one of those is reachable: [`DELTA_OWNED_THREAD_FIELDS`] holds `cwd`
//! alone, so `sandbox`, `approvalPolicy` and `config` all pass the launch's
//! rejection check and reach the server.
//!
//! [`DELTA_OWNED_THREAD_FIELDS`]: super::DELTA_OWNED_THREAD_FIELDS
//! [`LaunchOptionDangerPolicy`]: delta_usecase::LaunchOptionDangerPolicy

use serde_json::Value;

use super::config_merge::stated_settings;
use super::{thread_start_value, CONFIG_FIELD};

/// The `thread/start` field selecting the sandbox policy.
const SANDBOX_FIELD: &str = "sandbox";
/// The `thread/start` field selecting the approval policy.
const APPROVAL_POLICY_FIELD: &str = "approvalPolicy";

/// The `SandboxMode` member that grants the agent the whole machine.
const DANGER_FULL_ACCESS: &str = "danger-full-access";
/// The `AskForApproval` member that stops the server asking at all.
const APPROVAL_NEVER: &str = "never";

/// The `config.toml` key for the sandbox policy, as it appears inside a `config`
/// value (the `thread/start` field is `sandbox`; the config file spells it
/// `sandbox_mode`).
const SANDBOX_MODE_KEY: &str = "sandbox_mode";
/// The `config.toml` key for the approval policy, inside a `config` value.
const APPROVAL_POLICY_KEY: &str = "approval_policy";

/// The `AskForApproval` object branch's wrapper key
/// (`{"granular": {…}}`), per the vendored v2 schema.
const GRANULAR_KEY: &str = "granular";
/// The granular gates whose being *off* is what makes the granular form as
/// permissive as `never`: `sandbox_approval` stops the server asking before it
/// steps outside the sandbox, and `rules` stops it consulting the user's own
/// approval rules.
const GRANULAR_SAFETY_GATES: &[&str] = &["sandbox_approval", "rules"];

/// Whether a Codex launch option `(name, value)` disables the codex server's own
/// sandbox or approval gate.
///
/// `name` is a `thread/start` field and `value` its registry text, exactly as
/// stored. The text is mapped to the JSON value the launch would actually send
/// through [`thread_start_value`] — the same function `thread_start_params` uses
/// — so what is classified here is what the server would receive, not a second
/// reading of the same string.
pub fn is_dangerous_launch_option(name: &str, value: Option<&str>) -> bool {
    match name {
        SANDBOX_FIELD => thread_start_value(value) == Value::String(DANGER_FULL_ACCESS.to_owned()),
        APPROVAL_POLICY_FIELD => is_dangerous_approval_policy(&thread_start_value(value)),
        CONFIG_FIELD => config_states_a_bypass(&thread_start_value(value)),
        _ => false,
    }
}

/// Whether an `approvalPolicy` value stops the server asking.
///
/// The string form is the plain case (`never`). The object form is
/// `{"granular": {"sandbox_approval": …, "rules": …, …}}`, dangerous when either
/// of [`GRANULAR_SAFETY_GATES`] is switched off.
///
/// Anything else non-string is treated as dangerous **conservatively**: the
/// schema gives `AskForApproval` exactly those two shapes, so a third one is
/// either a shape Delta has not learned yet or a typo, and mis-marking such a
/// value costs the user one un-tickable "default" checkbox while mis-clearing it
/// would silently disarm every session. (A valueless `approvalPolicy` maps to
/// JSON `true` and lands here too — a value the server would refuse anyway.)
fn is_dangerous_approval_policy(value: &Value) -> bool {
    match value {
        Value::String(text) => text == APPROVAL_NEVER,
        Value::Object(object) => match object.get(GRANULAR_KEY).and_then(Value::as_object) {
            Some(granular) => GRANULAR_SAFETY_GATES
                .iter()
                .any(|gate| granular.get(*gate) == Some(&Value::Bool(false))),
            // An object that is not the granular form is the unknown shape the
            // conservative rule above is about.
            None => true,
        },
        _ => true,
    }
}

/// Whether a `config` value states a bypass of the sandbox or the approval gate.
///
/// Flattened through the launch's own [`stated_settings`], so the nested table
/// (`{"sandbox_mode": …}`) and the dotted key
/// (`{"profiles.work.sandbox_mode": …}`) reduce to the same canonical path and
/// neither spelling can slip past.
///
/// Each flattened setting is judged by [`setting_states_a_bypass`], which reads
/// the *tail* of the path rather than the whole of it, because Codex reads these
/// keys at more than one place: `sandbox_mode` at the top level and again inside
/// every `profiles.<name>` table. A profile's value only applies when that
/// profile is selected, so this is deliberately over-inclusive — stating the
/// bypass anywhere in a `config` row is enough to mark the row.
///
/// A `config` value that is not an object states nothing (the launch passes a
/// single non-object selection through verbatim and lets the server refuse it),
/// so it is not dangerous.
fn config_states_a_bypass(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    stated_settings(object)
        .into_iter()
        .any(|(path, value)| setting_states_a_bypass(&path, &value))
}

/// Whether one flattened `config` setting states a bypass.
///
/// `config.toml`'s `approval_policy` is the very same `AskForApproval` type as
/// the `approvalPolicy` thread field, so it has the same two shapes — and
/// [`stated_settings`] has already walked the object one down to its leaves.
/// A granular policy inside a `config` value therefore never arrives as a value
/// to compare against `never`; it arrives as `approval_policy.granular.<gate> =
/// false`, which is why that shape is matched on the path rather than on the
/// value. Missing it would have left the object form dangerous when written on
/// the thread field and invisible when written the other way — the exact
/// spelling-blindness this module exists to close.
fn setting_states_a_bypass(path: &[String], value: &Value) -> bool {
    // The policy stated as an object: `approval_policy` is a non-final segment,
    // and what follows it says which shape.
    if let Some(inner) = approval_policy_object_tail(path) {
        return match inner {
            [granular, gate]
                if granular == GRANULAR_KEY && GRANULAR_SAFETY_GATES.contains(&gate.as_str()) =>
            {
                value == &Value::Bool(false)
            }
            // A granular gate this module does not classify (`mcp_elicitations`
            // and friends) leaves the two that matter to their own leaves.
            [granular, _] if granular == GRANULAR_KEY => false,
            // Any other shape is the unknown one [`is_dangerous_approval_policy`]
            // marks conservatively; the two must agree, or the same value would
            // be dangerous on the thread field and benign inside `config`.
            _ => true,
        };
    }
    match path.last().map(String::as_str) {
        Some(SANDBOX_MODE_KEY) => value == &Value::String(DANGER_FULL_ACCESS.to_owned()),
        Some(APPROVAL_POLICY_KEY) => value == &Value::String(APPROVAL_NEVER.to_owned()),
        _ => false,
    }
}

/// The segments *below* an `approval_policy` table in a flattened path, or `None`
/// when the path does not run through one (`approval_policy` as the leaf is the
/// string form, handled by its own arm).
///
/// Matched anywhere in the path for the same reason the leaf match is: Codex
/// reads the key at the top level and again inside every `profiles.<name>`
/// table.
fn approval_policy_object_tail(path: &[String]) -> Option<&[String]> {
    let index = path
        .iter()
        .position(|segment| segment == APPROVAL_POLICY_KEY)?;
    let tail = &path[index + 1..];
    (!tail.is_empty()).then_some(tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_danger_full_access_sandbox_is_dangerous() {
        assert!(is_dangerous_launch_option(
            SANDBOX_FIELD,
            Some(DANGER_FULL_ACCESS)
        ));
        for benign in ["read-only", "workspace-write"] {
            assert!(!is_dangerous_launch_option(SANDBOX_FIELD, Some(benign)));
        }
    }

    #[test]
    fn approval_policy_never_is_dangerous_and_on_request_is_not() {
        assert!(is_dangerous_launch_option(
            APPROVAL_POLICY_FIELD,
            Some(APPROVAL_NEVER)
        ));
        for benign in ["untrusted", "on-request"] {
            assert!(!is_dangerous_launch_option(
                APPROVAL_POLICY_FIELD,
                Some(benign)
            ));
        }
    }

    /// The granular object form is dangerous exactly when a safety gate is off.
    #[test]
    fn the_granular_approval_policy_is_dangerous_when_a_gate_is_off() {
        let granular = |body: &str| format!(r#"{{"granular": {body}}}"#);
        for off in [
            r#"{"sandbox_approval": false, "rules": true, "mcp_elicitations": true}"#,
            r#"{"sandbox_approval": true, "rules": false, "mcp_elicitations": true}"#,
        ] {
            let raw = granular(off);
            assert!(
                is_dangerous_launch_option(APPROVAL_POLICY_FIELD, Some(&raw)),
                "a granular policy with a gate off is a bypass: {raw}"
            );
        }
        let all_on =
            granular(r#"{"sandbox_approval": true, "rules": true, "mcp_elicitations": true}"#);
        assert!(!is_dangerous_launch_option(
            APPROVAL_POLICY_FIELD,
            Some(&all_on)
        ));
    }

    /// An `approvalPolicy` shape the schema does not describe is marked rather
    /// than waved through.
    #[test]
    fn an_unknown_approval_policy_shape_is_conservatively_dangerous() {
        assert!(is_dangerous_launch_option(
            APPROVAL_POLICY_FIELD,
            Some(r#"{"whatever": 1}"#)
        ));
        assert!(is_dangerous_launch_option(APPROVAL_POLICY_FIELD, None));
    }

    /// Both of Codex's spellings of one `config` setting are caught, because the
    /// check runs on the canonical path the launch's own flatten produces.
    #[test]
    fn detects_danger_full_access_inside_a_config_value() {
        let nested = r#"{"sandbox_mode": "danger-full-access"}"#;
        let dotted = r#"{"profiles.work.sandbox_mode": "danger-full-access"}"#;
        let deeply_nested = r#"{"profiles": {"work": {"sandbox_mode": "danger-full-access"}}}"#;
        for raw in [nested, dotted, deeply_nested] {
            assert!(
                is_dangerous_launch_option(CONFIG_FIELD, Some(raw)),
                "a `config` row stating the full-access sandbox is dangerous however \
                 it is spelled: {raw}"
            );
        }
    }

    #[test]
    fn detects_approval_policy_never_inside_a_config_value() {
        for raw in [
            r#"{"approval_policy": "never"}"#,
            r#"{"profiles": {"work": {"approval_policy": "never"}}}"#,
        ] {
            assert!(is_dangerous_launch_option(CONFIG_FIELD, Some(raw)), "{raw}");
        }
    }

    /// `config.toml`'s `approval_policy` is the same `AskForApproval` type as the
    /// thread field, so its object form has to be caught inside a `config` value
    /// too — where the flatten has already reduced it to
    /// `approval_policy.granular.<gate>` leaves.
    #[test]
    fn detects_a_granular_approval_policy_inside_a_config_value() {
        for raw in [
            r#"{"approval_policy": {"granular": {"sandbox_approval": false, "rules": true, "mcp_elicitations": true}}}"#,
            r#"{"approval_policy": {"granular": {"sandbox_approval": true, "rules": false, "mcp_elicitations": true}}}"#,
            r#"{"approval_policy.granular.rules": false}"#,
            r#"{"profiles": {"work": {"approval_policy": {"granular": {"sandbox_approval": false, "rules": true, "mcp_elicitations": true}}}}}"#,
            // A shape the schema does not describe: marked, exactly as the
            // thread field marks it.
            r#"{"approval_policy": {"whatever": 1}}"#,
        ] {
            assert!(is_dangerous_launch_option(CONFIG_FIELD, Some(raw)), "{raw}");
        }
    }

    #[test]
    fn an_ordinary_config_value_is_not_dangerous() {
        for raw in [
            r#"{"model_reasoning_summary": "auto"}"#,
            r#"{"sandbox_mode": "workspace-write"}"#,
            r#"{"sandbox_workspace_write": {"writable_roots": ["/tmp"]}}"#,
            // Every gate on is the granular form asking to be *asked*.
            r#"{"approval_policy": {"granular": {"sandbox_approval": true, "rules": true, "mcp_elicitations": true}}}"#,
            r#"{"approval_policy": "on-request"}"#,
            // Not an object: the launch sends it verbatim and the server refuses
            // it, so there is no setting here to classify.
            "not json at all",
        ] {
            assert!(
                !is_dangerous_launch_option(CONFIG_FIELD, Some(raw)),
                "{raw}"
            );
        }
    }

    #[test]
    fn an_ordinary_field_is_not_dangerous() {
        assert!(!is_dangerous_launch_option("model", Some("gpt-5")));
        assert!(!is_dangerous_launch_option(
            "approvalsReviewer",
            Some("auto_review")
        ));
    }
}
