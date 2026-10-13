//! The `thread/start` and `thread/resume` params a launch and a resume send.

use serde_json::{json, Map, Value};

use delta_usecase::{
    Error as UsecaseError, LaunchOptionCardinality, LaunchOptionSpec, Result as UsecaseResult,
};

#[cfg(doc)]
use delta_usecase::AgentAdapter;

use super::config_merge::{merge_config, ConfigSelection};
use super::worktree_git_grant::apply_worktree_git_grant;
use super::{launch_option_cardinality, CONFIG_FIELD, DELTA_OWNED_THREAD_FIELDS};

/// Build the `thread/start` params for a launch: Delta's own fields plus the
/// user's selected launch options, mapped one-to-one onto `ThreadStartParams`
/// fields.
///
/// A launch option is passed through **unvalidated**: its `name` is the field
/// name and its `value` is that field's value. `ThreadStartParams` is a moving
/// target (`model`, `sandbox`, `approvalPolicy`, `personality`, `serviceTier`,
/// `config`, …), so an allowlist here would mean a Delta release for every new
/// upstream field. The cost of the pass-through is that a misspelled key or a
/// bad value is not caught here — it comes back as an error from the codex
/// server, which is where the authority over that schema actually lives.
///
/// Two things are still rejected, loudly, because silently accepting them would
/// corrupt state Delta is responsible for:
///
/// - a key Delta sets itself (`DELTA_OWNED_THREAD_FIELDS`). `cwd` is
///   load-bearing: with a worktree it is the resolved worktree path, and the
///   session's repo-root / display-name / branch-at-launch columns are recorded
///   against it, so a user-registered `cwd` overriding it would break the
///   worktree contract while the recorded columns went on describing a
///   directory the agent is not in;
/// - the same key twice — with one exception. Unlike a repeatable CLI flag, a
///   JSON field can only be set once, so a second option carrying the same name
///   would silently discard the first. The exception is `config`, which is not
///   one setting but an object holding many: two `config` options are
///   **deep-merged** (see `config_merge::merge_config`), and only a genuine
///   disagreement between them — two values for one setting — is rejected, with
///   every such disagreement reported together. Which names are single-valued
///   is [`launch_option_cardinality()`], the same classification the domain
///   enforces before a launch reaches this builder; the check stays here too so
///   the builder is correct on its own (the canary lane drives it directly).
///
/// `worktree_repo_root`, when the session runs in a Delta-created worktree,
/// contributes the sandbox grant for that worktree's real git directory (see
/// `apply_worktree_git_grant`). It is applied **after** the user's options are
/// merged, so it sees the one `config` object they add up to and unions its
/// writable root into it. With no worktree the params are exactly what they were
/// before the grant existed.
///
/// It is `pub` so the real-`codex` canary lane can drive the very builder the
/// adapter launches with, instead of re-spelling the params in the test and
/// pinning a shape production never sends.
pub fn thread_start_params(
    workdir: &str,
    options: &[LaunchOptionSpec],
    worktree_repo_root: Option<&str>,
) -> UsecaseResult<Value> {
    let mut params = Map::new();
    params.insert("cwd".to_owned(), json!(workdir));
    // Held aside in selection order and merged once the loop is done, because
    // `config` is the one field a launch may select more than once.
    let mut config_selections: Vec<ConfigSelection> = Vec::new();
    for option in options {
        if DELTA_OWNED_THREAD_FIELDS.contains(&option.name.as_str()) {
            return Err(UsecaseError::LaunchOptionRejected(format!(
                "`{}` cannot be used with Codex: Delta sets that thread/start \
                 field itself for every session",
                option.name
            )));
        }
        // `config` is the one name [`launch_option_cardinality`] classifies as
        // repeatable; every other name is a single-valued field.
        if option.name == CONFIG_FIELD {
            config_selections.push(ConfigSelection {
                raw: option.value.clone(),
                value: thread_start_value(option.value.as_deref()),
            });
            continue;
        }
        if launch_option_cardinality(&option.name) == LaunchOptionCardinality::Single
            && params.contains_key(&option.name)
        {
            return Err(UsecaseError::LaunchOptionRejected(format!(
                "`{}` is selected more than once: a thread/start field can only \
                 be set once",
                option.name
            )));
        }
        params.insert(
            option.name.clone(),
            thread_start_value(option.value.as_deref()),
        );
    }
    if let Some(config) = merge_config(&config_selections)? {
        params.insert(CONFIG_FIELD.to_owned(), config);
    }
    apply_worktree_git_grant(&mut params, worktree_repo_root);
    Ok(Value::Object(params))
}

/// Build the `thread/resume` params for a reattach: the thread being resumed,
/// the directory to resume it in, and — for a Delta-created worktree — the same
/// git-directory grant a fresh start gets (see [`apply_worktree_git_grant`]).
///
/// No launch options ride a resume (see [`AgentAdapter::resume`]), so unlike
/// [`thread_start_params`] this can never fail: there is no user-supplied key to
/// reject. The grant is re-derived from the resume request, not remembered from
/// the launch, so the worktree's git directory is writable on a resumed session
/// exactly as on a fresh one, even across a server restart.
///
/// The rest of that launch's `config` does **not** come back with it. With no
/// launch options there is nothing to merge, so [`apply_worktree_git_grant`]
/// always takes its no-`config` branch and the resumed thread's
/// `writable_roots` are Delta's path alone — a user whose selected `config` row
/// listed roots of their own has them on the launch and not on the resume. That
/// is launch options being a launch-time concept rather than anything the union
/// changed, but it is why the union does not by itself make the two calls
/// equivalent.
///
/// One caveat on that grant surviving a restart: what the real server does with
/// a `config` on a **resume** is not pinned anywhere. The schema test only says the field
/// exists on both calls, and the canary drives `thread/start` alone — resuming
/// a thread the same connection just started rejoins a *running* thread, which
/// is not the after-a-restart path Delta takes, so it would prove the wrong
/// thing. The claim is settled by a real restart instead: `git add` inside a
/// worktree session whose server was restarted must still not prompt.
pub(crate) fn thread_resume_params(
    provider_session_id: &str,
    workdir: &str,
    worktree_repo_root: Option<&str>,
) -> Value {
    let mut params = Map::new();
    params.insert("threadId".to_owned(), json!(provider_session_id));
    params.insert("cwd".to_owned(), json!(workdir));
    apply_worktree_git_grant(&mut params, worktree_repo_root);
    Value::Object(params)
}

/// Map a launch option's registry value onto its `thread/start` field value.
///
/// The registry stores values as text, but `ThreadStartParams` fields are not
/// all strings — `ephemeral` is a boolean, `config` is an object, and
/// `approvalPolicy` is either a string or an object. So:
///
/// - **no value** → JSON `true`. A valueless option reads as a bare boolean
///   field being switched on (`ephemeral`), the same way a valueless CLI flag
///   reads on the Claude side;
/// - **a value that parses as JSON** → that JSON value. `true`, `42`,
///   `{"granular":{…}}` and `["a","b"]` all reach the server with their real
///   types;
/// - **anything else** → the value as a JSON string. This is the common case:
///   `gpt-5.6-sol`, `read-only` and `on-request` are not valid JSON documents,
///   so they arrive as the strings they are.
///
/// The consequence of that ordering is that a string value which *happens* to
/// be valid JSON (`5`, `null`, `true`) becomes the parsed type. A user who
/// needs the literal string writes it quoted (`"5"`), which parses as the JSON
/// string `5`.
pub(super) fn thread_start_value(value: Option<&str>) -> Value {
    match value {
        None => Value::Bool(true),
        Some(raw) => serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned())),
    }
}
