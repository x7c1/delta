//! Merging the `config` launch options a launch selected into the single
//! `thread/start` field they all land in.
//!
//! Every other `thread/start` field is one setting, so selecting two options
//! that name it is ambiguous and rejected (see [`super::thread_start_params`]).
//! `config` is the exception: it is one JSON *object* holding many independent
//! settings — the shipped `Config: reasoning summary` preset states
//! `model_reasoning_summary`, a user's own row typically states
//! `sandbox_workspace_write.writable_roots` — so two selections are two
//! different settings far more often than they are a contradiction. This module
//! merges them, and rejects only the cases where the user really did say two
//! things about one setting.
//!
//! Its own module because the merge is a self-contained value transformation
//! with its own vocabulary (paths, spellings, conflicts), while
//! [`super`] is about driving a live session.
//!
//! ## Paths and spellings
//!
//! Codex's config format lets one setting be named two ways: the nested table
//! (`{"sandbox_workspace_write": {"writable_roots": […]}}`) or a dotted key
//! (`{"sandbox_workspace_write.writable_roots": […]}`). Both name the same
//! **path** `sandbox_workspace_write.writable_roots`; they differ only in
//! **spelling**. So the merge works on paths — flattening each selection into
//! the settings it states — while remembering the spelling each setting was
//! written with, and rebuilds the merged object in those same spellings.
//!
//! Spelling is preserved rather than normalised because the dotted form is the
//! one the real-Codex canary asserts applies at the leaf (see the module docs of
//! [`super`], "The worktree git-directory grant"); rewriting a user's key into
//! the other form would be Delta guessing at semantics it has only measured for
//! one of them.
//!
//! ## What is rejected
//!
//! A conflict is two selections saying different things about one setting:
//! different scalars, a scalar against a list, two lists that are not the same
//! list, one selection's setting sitting *inside* another's (`a.b` against `a`),
//! or the same path written in both spellings — which is the user duplicating a
//! setting rather than adding one, and is reported instead of silently letting
//! one side win.
//!
//! The single exception is `sandbox_workspace_write.writable_roots`, whose two
//! lists are **unioned** rather than compared (see [`is_writable_roots`]) — a
//! set of paths is the one list shape where holding both is what the user meant.
//!
//! Every conflict found is collected and reported together: a user who
//! mis-copied a row wants the whole list, not one key per round-trip.

use serde_json::{Map, Value};

use delta_usecase::{Error as UsecaseError, Result as UsecaseResult};

use super::worktree_git_grant::{SANDBOX_WORKSPACE_WRITE, WRITABLE_ROOTS_LEAF};

/// One selected `config` launch option, as [`super::thread_start_params`] saw
/// it.
pub(super) struct ConfigSelection {
    /// The registry text the row carried, verbatim. Two `config` rows share the
    /// same `name` (and the adapter never sees their labels), so this is what
    /// tells the user *which* of their rows a rejection is about.
    pub raw: Option<String>,
    /// The value the row's text mapped to (see [`super::thread_start_value`]).
    pub value: Value,
}

impl ConfigSelection {
    /// How a rejection names this row: its registered value, which is the only
    /// thing that distinguishes one selected `config` row from another here.
    fn describe(&self) -> String {
        match &self.raw {
            Some(raw) => format!("`config` = `{raw}`"),
            None => "the valueless `config` option".to_owned(),
        }
    }
}

/// One setting a `config` selection states: the path it names, the key chain it
/// was written with, and the value it was given.
struct Stated {
    /// The setting's canonical path — dotted keys split, nested tables walked —
    /// so that both spellings of one setting compare equal.
    path: Vec<String>,
    /// The keys as the user actually wrote them, outermost first. Rebuilding
    /// through this chain is what keeps a dotted key dotted and a nested table
    /// nested.
    spelling: Vec<String>,
    value: Value,
}

/// The merged `config` value for a launch, or `None` when no `config` option was
/// selected.
///
/// A single selection is passed through **verbatim**, whatever it is: the launch
/// path does not validate a launch option's value (the codex server is the
/// authority on it), and that stays true for one `config` — including one that
/// is not an object at all. Only from two selections on does merging require
/// them to be objects, because there is no other way to combine them.
pub(super) fn merge_config(selections: &[ConfigSelection]) -> UsecaseResult<Option<Value>> {
    match selections {
        [] => Ok(None),
        [only] => Ok(Some(only.value.clone())),
        many => merge_many(many).map(Some),
    }
}

/// Merge two or more selections, in selection order.
fn merge_many(selections: &[ConfigSelection]) -> UsecaseResult<Value> {
    let non_objects: Vec<String> = selections
        .iter()
        .filter(|selection| !selection.value.is_object())
        .map(ConfigSelection::describe)
        .collect();
    let complaint = match non_objects.as_slice() {
        [] => None,
        [only] => Some(format!("{only} is not a JSON object")),
        several => Some(format!("{} are not JSON objects", several.join(", "))),
    };
    if let Some(complaint) = complaint {
        return Err(UsecaseError::LaunchOptionRejected(format!(
            "{} `config` options are selected, so they are merged into one \
             object — but {complaint}",
            selections.len()
        )));
    }

    let mut merged: Vec<Stated> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();
    for selection in selections {
        let object = selection
            .value
            .as_object()
            .expect("every selection was checked to be an object above");
        for stated in flatten(object) {
            absorb(&mut merged, stated, &mut conflicts);
        }
    }
    if !conflicts.is_empty() {
        return Err(UsecaseError::LaunchOptionRejected(format!(
            "the selected `config` options disagree: {}",
            conflicts.join("; ")
        )));
    }
    Ok(rebuild(merged))
}

/// Fold one stated setting into the settings merged so far, recording a conflict
/// rather than letting either side win.
fn absorb(merged: &mut Vec<Stated>, incoming: Stated, conflicts: &mut Vec<String>) {
    for existing in merged.iter_mut() {
        if existing.path == incoming.path {
            if existing.spelling != incoming.spelling {
                conflicts.push(format!(
                    "`{}` is stated twice, once as {} and once as {}",
                    incoming.path.join("."),
                    describe_spelling(&existing.spelling),
                    describe_spelling(&incoming.spelling)
                ));
                return;
            }
            match merge_values(&incoming.path, &existing.value, &incoming.value) {
                Some(value) => existing.value = value,
                None => conflicts.push(format!(
                    "`{}` is set to both {} and {}",
                    incoming.path.join("."),
                    existing.value,
                    incoming.value
                )),
            }
            return;
        }
        if is_prefix(&existing.path, &incoming.path) || is_prefix(&incoming.path, &existing.path) {
            conflicts.push(format!(
                "`{}` and `{}` cannot both be set: one is inside the other",
                existing.path.join("."),
                incoming.path.join(".")
            ));
            return;
        }
    }
    merged.push(incoming);
}

/// Render a key chain as the user wrote it, in bracket notation
/// (`["sandbox_workspace_write"]["writable_roots"]` for the nested table,
/// `["sandbox_workspace_write.writable_roots"]` for the dotted key).
///
/// Dotted joining would render both spellings of one setting identically, which
/// is exactly the pair this appears in a message about; brackets keep the two
/// legible side by side.
fn describe_spelling(spelling: &[String]) -> String {
    spelling
        .iter()
        .map(|key| format!("[{}]", Value::String(key.clone())))
        .collect()
}

/// Whether `shorter` names an ancestor of `longer` (a strict prefix of its
/// path). Equal paths are not prefixes of each other — that case is the
/// same-setting one, handled before this is consulted.
fn is_prefix(shorter: &[String], longer: &[String]) -> bool {
    shorter.len() < longer.len() && longer.starts_with(shorter)
}

/// The merged value of one setting stated by two selections, or `None` when they
/// disagree.
///
/// Only leaf values reach here — [`flatten`] has already walked every non-empty
/// object — so this is the scalar/list half of the rules: two `writable_roots`
/// lists union with the earlier selection first (see [`is_writable_roots`]),
/// dropping entries the earlier list already carries so a shared path is not
/// listed twice; two equal values are simply that value; anything else — two
/// lists under any *other* path included — is the user saying two different
/// things.
fn merge_values(path: &[String], earlier: &Value, later: &Value) -> Option<Value> {
    match (earlier, later) {
        (Value::Array(earlier), Value::Array(later)) if is_writable_roots(path) => {
            let mut merged = earlier.clone();
            for entry in later {
                if !merged.contains(entry) {
                    merged.push(entry.clone());
                }
            }
            Some(Value::Array(merged))
        }
        _ if earlier == later => Some(earlier.clone()),
        _ => None,
    }
}

/// Whether a path names `sandbox_workspace_write.writable_roots` — the one
/// setting whose two lists are unioned instead of having to match.
///
/// Writable roots are a **set** of paths: two rows each naming the roots their
/// machine needs mean both, and Delta's own worktree grant already appends to
/// that list (see [`super::worktree_git_grant`]). Every other Codex list setting
/// is a *sequence* — `mcp_servers.<name>.args` is the plain case — where
/// splicing two selections together yields a value neither row asked for, so
/// those must disagree loudly instead.
///
/// The test is on the canonical path, so it holds for both of Codex's spellings
/// without either being parsed a second time here: [`flatten`] has already split
/// a dotted key and walked a nested table into the same two segments.
fn is_writable_roots(path: &[String]) -> bool {
    matches!(
        path,
        [sandbox, roots]
            if sandbox == SANDBOX_WORKSPACE_WRITE && roots == WRITABLE_ROOTS_LEAF
    )
}

/// Flatten one `config` object into the settings it states, splitting dotted
/// keys and walking nested tables so that both spellings of one setting produce
/// the same path.
///
/// An **empty** table states nothing, so it produces no setting at all: it
/// carries no value to merge, and treating it as a leaf would make
/// `{"a": {}}` collide with another selection's `{"a": {"b": 1}}` over a
/// setting neither of them actually set.
fn flatten(object: &Map<String, Value>) -> Vec<Stated> {
    let mut out = Vec::new();
    walk(object, &[], &[], &mut out);
    out
}

fn walk(object: &Map<String, Value>, path: &[String], spelling: &[String], out: &mut Vec<Stated>) {
    for (key, value) in object {
        let mut child_path = path.to_vec();
        child_path.extend(key.split('.').map(str::to_owned));
        let mut child_spelling = spelling.to_vec();
        child_spelling.push(key.clone());
        match value {
            Value::Object(inner) if !inner.is_empty() => {
                walk(inner, &child_path, &child_spelling, out);
            }
            Value::Object(_) => {}
            _ => out.push(Stated {
                path: child_path,
                spelling: child_spelling,
                value: value.clone(),
            }),
        }
    }
}

/// Rebuild a `config` object from merged settings, each written back through the
/// key chain it arrived with.
///
/// The settings are pairwise non-overlapping by construction (an equal path is
/// merged and an ancestor/descendant pair is rejected), so no chain can collide
/// with another's value.
fn rebuild(settings: Vec<Stated>) -> Value {
    let mut root = Map::new();
    for setting in settings {
        let (last, parents) = setting
            .spelling
            .split_last()
            .expect("a stated setting always has at least one key");
        let mut table = &mut root;
        for parent in parents {
            table = table
                .entry(parent.clone())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .expect("merged settings never nest inside another's value");
        }
        table.insert(last.clone(), setting.value);
    }
    Value::Object(root)
}
