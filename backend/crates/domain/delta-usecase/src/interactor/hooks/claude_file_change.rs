//! The change set of a Claude file-editing permission request, read from the
//! `PermissionRequest` hook's own tool input.
//!
//! Codex states a proposed patch on a separate item that the adapter
//! correlates with the approval. Claude's file tools carry the whole change in
//! their input instead — the new content of a `Write`, the `old_string` /
//! `new_string` pairs of an `Edit` or `MultiEdit`, the new cell source of a
//! `NotebookEdit` — so the same [`AgentFileChangeDetail`] the card already
//! renders for Codex can be built straight from that input, and the card shows
//! the change rather than only the arguments that describe it.
//!
//! The diff text is a rendering of the input, not a diff against the file on
//! disk: Delta does not read the file, so an `Edit`'s hunk is the replacement
//! exactly as requested (`-` lines for `old_string`, `+` lines for
//! `new_string`) with a header naming the edit and whether it replaces every
//! occurrence.

use serde_json::Value;

use crate::agent::{AgentFileChange, AgentFileChangeDetail, AgentFileChangeKind};

/// Claude Code's whole-file write tool.
const WRITE: &str = "Write";
/// Claude Code's single string-replacement tool.
const EDIT: &str = "Edit";
/// Claude Code's batched string-replacement tool.
const MULTI_EDIT: &str = "MultiEdit";
/// Claude Code's Jupyter notebook cell tool.
const NOTEBOOK_EDIT: &str = "NotebookEdit";

/// The change a Claude file tool's permission request would make, or `None`
/// for any other tool and for an input that does not name its file.
///
/// A file tool whose input lacks its path yields `None` rather than an entry
/// with an empty path: the change view's job is to say which file is affected,
/// and the card still lists every argument as it is. `reason` is always
/// `None` — Claude's hook gives no explanation beside the input.
pub(in crate::interactor) fn claude_file_change(
    tool_name: &str,
    input: &Value,
) -> Option<AgentFileChangeDetail> {
    let change = match tool_name {
        WRITE => AgentFileChange {
            path: string(input, "file_path")?,
            // Write creates or overwrites; which one depends on the disk, which
            // Delta does not read, so no kind is claimed.
            kind: None,
            diff: prefixed_lines('+', &string(input, "content").unwrap_or_default()),
        },
        EDIT => AgentFileChange {
            path: string(input, "file_path")?,
            kind: Some(AgentFileChangeKind::Update),
            diff: replacement_hunk(input, None),
        },
        MULTI_EDIT => {
            let path = string(input, "file_path")?;
            let edits = input
                .get("edits")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let hunks: Vec<String> = edits
                .iter()
                .enumerate()
                .map(|(index, edit)| replacement_hunk(edit, Some((index + 1, edits.len()))))
                .collect();
            AgentFileChange {
                path,
                kind: Some(AgentFileChangeKind::Update),
                diff: hunks.join("\n"),
            }
        }
        NOTEBOOK_EDIT => AgentFileChange {
            path: string(input, "notebook_path")?,
            kind: Some(AgentFileChangeKind::Update),
            diff: notebook_hunk(input),
        },
        _ => return None,
    };
    Some(AgentFileChangeDetail {
        changes: vec![change],
        reason: None,
    })
}

/// One `old_string` → `new_string` replacement as a hunk: a header naming the
/// edit (its position among several, and whether it replaces every
/// occurrence), the old text as `-` lines and the new text as `+` lines.
fn replacement_hunk(edit: &Value, position: Option<(usize, usize)>) -> String {
    let every = edit
        .get("replace_all")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let label = match position {
        Some((index, total)) => format!("edit {index} of {total}"),
        None => "edit".to_owned(),
    };
    let scope = if every { ", every occurrence" } else { "" };
    let mut lines = vec![format!("@@ {label}{scope} @@")];
    lines.extend(prefixed(
        '-',
        &string(edit, "old_string").unwrap_or_default(),
    ));
    lines.extend(prefixed(
        '+',
        &string(edit, "new_string").unwrap_or_default(),
    ));
    lines.join("\n")
}

/// A notebook cell edit as a hunk: a header naming the edit mode and the cell,
/// then the new source as `+` lines (none for a deletion).
fn notebook_hunk(input: &Value) -> String {
    let mode = string(input, "edit_mode").unwrap_or_else(|| "replace".to_owned());
    let cell = string(input, "cell_id")
        .map(|id| format!(" cell {id}"))
        .unwrap_or_default();
    let mut lines = vec![format!("@@ {mode}{cell} @@")];
    lines.extend(prefixed(
        '+',
        &string(input, "new_source").unwrap_or_default(),
    ));
    lines.join("\n")
}

/// `text` as diff lines, each prefixed with `marker`, joined by newlines.
fn prefixed_lines(marker: char, text: &str) -> String {
    prefixed(marker, text).collect::<Vec<_>>().join("\n")
}

/// Each line of `text` prefixed with `marker`. An empty text has no lines; a
/// trailing newline shows as a final empty marked line, so it is not lost.
fn prefixed(marker: char, text: &str) -> impl Iterator<Item = String> + '_ {
    (!text.is_empty())
        .then(|| text.split('\n'))
        .into_iter()
        .flatten()
        .map(move |line| format!("{marker}{line}"))
}

/// The string at `key`, if it is one.
fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn only_change(tool_name: &str, input: Value) -> AgentFileChange {
        let detail = claude_file_change(tool_name, &input).expect("a file tool has a change");
        assert_eq!(detail.reason, None);
        let [change] = <[AgentFileChange; 1]>::try_from(detail.changes).expect("one change");
        change
    }

    #[test]
    fn write_shows_the_new_content() {
        let change = only_change(
            WRITE,
            json!({"file_path": "/repo/notes.txt", "content": "first\nsecond"}),
        );
        assert_eq!(change.path, "/repo/notes.txt");
        assert_eq!(change.kind, None);
        assert_eq!(change.diff, "+first\n+second");
    }

    #[test]
    fn edit_shows_the_old_and_new_strings() {
        let change = only_change(
            EDIT,
            json!({
                "file_path": "/repo/src/lib.rs",
                "old_string": "let a = 1;",
                "new_string": "let a = 2;\nlet b = 3;"
            }),
        );
        assert_eq!(change.path, "/repo/src/lib.rs");
        assert_eq!(change.kind, Some(AgentFileChangeKind::Update));
        assert_eq!(
            change.diff,
            "@@ edit @@\n-let a = 1;\n+let a = 2;\n+let b = 3;"
        );
    }

    #[test]
    fn edit_states_a_replacement_of_every_occurrence() {
        let change = only_change(
            EDIT,
            json!({
                "file_path": "a.rs",
                "old_string": "x",
                "new_string": "y",
                "replace_all": true
            }),
        );
        assert_eq!(change.diff, "@@ edit, every occurrence @@\n-x\n+y");
    }

    #[test]
    fn multi_edit_shows_each_pair_in_order() {
        let change = only_change(
            MULTI_EDIT,
            json!({
                "file_path": "a.rs",
                "edits": [
                    {"old_string": "one", "new_string": "uno"},
                    {"old_string": "two", "new_string": "dos", "replace_all": true}
                ]
            }),
        );
        assert_eq!(change.kind, Some(AgentFileChangeKind::Update));
        assert_eq!(
            change.diff,
            "@@ edit 1 of 2 @@\n-one\n+uno\n@@ edit 2 of 2, every occurrence @@\n-two\n+dos"
        );
    }

    #[test]
    fn notebook_edit_shows_the_new_cell_source() {
        let change = only_change(
            NOTEBOOK_EDIT,
            json!({
                "notebook_path": "/repo/n.ipynb",
                "cell_id": "c1",
                "new_source": "print(1)",
                "edit_mode": "insert"
            }),
        );
        assert_eq!(change.path, "/repo/n.ipynb");
        assert_eq!(change.diff, "@@ insert cell c1 @@\n+print(1)");
    }

    #[test]
    fn a_trailing_newline_is_kept_as_an_empty_line() {
        let change = only_change(WRITE, json!({"file_path": "a", "content": "x\n"}));
        assert_eq!(change.diff, "+x\n+");
    }

    #[test]
    fn other_tools_and_pathless_inputs_have_no_change() {
        assert_eq!(claude_file_change("Bash", &json!({"command": "ls"})), None);
        assert_eq!(claude_file_change(WRITE, &json!({"content": "x"})), None);
        assert_eq!(claude_file_change(EDIT, &Value::Null), None);
    }
}
