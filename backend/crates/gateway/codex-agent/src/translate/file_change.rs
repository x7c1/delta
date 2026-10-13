//! The change set of a file-change item, which the adapter correlates with the
//! approval raised for that item.

use serde_json::Value;

use delta_usecase::{
    AgentFileChange, AgentFileChangeDetail, AgentFileChangeKind, AgentPermissionRequest,
};

use crate::wire::Notification;

use super::{item_of, string_field, FILE_CHANGE_ITEM_TYPE, METHOD_FILE_CHANGE_PATCH_UPDATED};

/// The `FileUpdateChange` field naming the path that changes.
const CHANGE_PATH_FIELD: &str = "path";
/// The `FileUpdateChange` field carrying the unified diff of the change.
const CHANGE_DIFF_FIELD: &str = "diff";
/// The `FileUpdateChange` field carrying the change's kind — a
/// `PatchChangeKind`, i.e. an *object* whose `type` is the discriminant, not a
/// bare string.
const CHANGE_KIND_FIELD: &str = "kind";
/// The `PatchChangeKind` discriminant field (`add` / `delete` / `update`).
const CHANGE_KIND_TYPE_FIELD: &str = "type";
/// The array of `FileUpdateChange`s, carried both by a `FileChangeThreadItem`
/// and by [`METHOD_FILE_CHANGE_PATCH_UPDATED`] under the same key.
const CHANGES_FIELD: &str = "changes";

/// Complete a file-change approval with the change set correlated from its item,
/// so the browser card can name the affected files and show their diffs.
///
/// The `reason` comes from the approval's own params (which already ride
/// `input_json`) while the changes come from the item — the two halves of the
/// same question, joined here. Called only with changes an item actually stated;
/// a request left untouched keeps `file_change: None` and falls back to the input
/// summary.
pub fn with_file_change_detail(
    request: &mut AgentPermissionRequest,
    changes: Vec<AgentFileChange>,
) {
    let reason = string_field(&request.input_json, "reason");
    request.file_change = Some(AgentFileChangeDetail { changes, reason });
}

/// The file-change item a notification announces, as `(item id, changes)` — the
/// pair the adapter keys its correlation map by.
///
/// Two notifications state a file-change item's proposed patch, and both are
/// read here so the map is correct at the moment an approval arrives:
///
/// - `item/started` for a `fileChange` item (`params.item`, a
///   `FileChangeThreadItem`) — the first statement of the patch;
/// - [`METHOD_FILE_CHANGE_PATCH_UPDATED`] (`params.itemId` / `params.changes`) —
///   a **replacement** of it, so the caller overwrites rather than merges.
///
/// `None` for every other notification, for an item of another type, and for an
/// item with no id: nothing to correlate, so nothing is tracked.
pub fn file_change_item(n: &Notification) -> Option<(String, Vec<AgentFileChange>)> {
    let (id, changes) = match n.method.as_str() {
        "item/started" => {
            let item = item_of(&n.params)?;
            if string_field(item, "type").as_deref() != Some(FILE_CHANGE_ITEM_TYPE) {
                return None;
            }
            (string_field(item, "id")?, item.get(CHANGES_FIELD))
        }
        METHOD_FILE_CHANGE_PATCH_UPDATED => (
            string_field(&n.params, "itemId")?,
            n.params.get(CHANGES_FIELD),
        ),
        _ => return None,
    };
    Some((id, file_changes(changes)))
}

/// The id of the item an `item/completed` announces, so the adapter can drop the
/// correlation entry the item no longer needs. `None` for any other notification
/// (or a completed item with no id).
pub fn completed_item_id(n: &Notification) -> Option<String> {
    match n.method.as_str() {
        "item/completed" => string_field(item_of(&n.params)?, "id"),
        _ => None,
    }
}

/// Project a `FileUpdateChange` array into the neutral change list.
///
/// An entry missing its required `path` is skipped: a change Delta cannot name
/// is worse than no row at all, since the card's whole job is to say which file
/// is affected. A missing `diff` degrades to the empty string (the expander then
/// has nothing to show) rather than dropping the path.
fn file_changes(changes: Option<&Value>) -> Vec<AgentFileChange> {
    let Some(changes) = changes.and_then(Value::as_array) else {
        return Vec::new();
    };
    changes
        .iter()
        .filter_map(|change| {
            Some(AgentFileChange {
                path: string_field(change, CHANGE_PATH_FIELD)?,
                kind: file_change_kind(change.get(CHANGE_KIND_FIELD)),
                diff: string_field(change, CHANGE_DIFF_FIELD).unwrap_or_default(),
            })
        })
        .collect()
}

/// Map a `PatchChangeKind` onto the neutral kind. The wire shape is an object
/// tagged by `type` (`{"type":"update","move_path":null}`), not a bare string.
/// A kind this build does not model — including an absent or malformed one —
/// yields `None`, so the path and diff still show without a fabricated label.
fn file_change_kind(kind: Option<&Value>) -> Option<AgentFileChangeKind> {
    match string_field(kind?, CHANGE_KIND_TYPE_FIELD)?.as_str() {
        "add" => Some(AgentFileChangeKind::Add),
        "update" => Some(AgentFileChangeKind::Update),
        "delete" => Some(AgentFileChangeKind::Delete),
        _ => None,
    }
}
