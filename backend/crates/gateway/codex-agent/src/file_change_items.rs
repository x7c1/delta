//! The per-session correlation between a Codex file-change **item** and the
//! approval request that gates it.
//!
//! ## Why this exists
//!
//! `item/fileChange/requestApproval` carries only
//! `{ itemId, startedAtMs, threadId, turnId, grantRoot?, reason? }` — no path,
//! no kind, no diff. The information the user needs to answer the prompt
//! travelled a moment earlier, on the `item/started` for the *same* `itemId`,
//! as a `FileChangeThreadItem`'s `changes` array. Nothing on the wire joins the
//! two; this map is that join, and it is kept here — at Codex's own edge —
//! rather than in the neutral core or the browser, because the split params are
//! this provider's wire quirk and nobody else's.
//!
//! ## Lifetime
//!
//! An entry is a *live* fact about a patch that has not been answered yet, so
//! it is dropped as soon as it stops being one: when the item completes, when
//! the turn ends, and when the connection is lost. Without that, a session
//! running for hours would accumulate every diff it ever proposed — the diffs
//! are the bulky part — for the process's lifetime.
//!
//! The entry is deliberately **not** dropped when the approval is answered:
//! `resolve_permission` runs on the adapter's request path with no item id in
//! hand (it is keyed by the neutral request id), and the item's own completion
//! follows the decision immediately anyway. The lifecycle points above cover it.

use std::collections::HashMap;

use delta_usecase::AgentFileChange;

/// The file-change items one session is currently tracking, keyed by the Codex
/// item id an approval request names.
#[derive(Debug, Default)]
pub(crate) struct FileChangeItems {
    items: HashMap<String, Vec<AgentFileChange>>,
}

impl FileChangeItems {
    /// Record (or replace) the change set an item proposes.
    ///
    /// Replacement is the point rather than a side effect:
    /// `item/fileChange/patchUpdated` restates the whole array, so an item whose
    /// patch was revised must forget the version `item/started` announced.
    pub(crate) fn record(&mut self, item_id: String, changes: Vec<AgentFileChange>) {
        self.items.insert(item_id, changes);
    }

    /// The change set tracked for an item, if it is still known. `None` is the
    /// honest answer for an item that was never seen, already completed, or
    /// belonged to a finished turn — the caller falls back to the request's own
    /// params rather than showing an empty detail.
    pub(crate) fn get(&self, item_id: &str) -> Option<Vec<AgentFileChange>> {
        self.items.get(item_id).cloned()
    }

    /// Drop one item's entry, once the item has completed.
    pub(crate) fn forget(&mut self, item_id: &str) {
        self.items.remove(item_id);
    }

    /// Drop every entry — at turn end and on connection loss, where no tracked
    /// patch can still be awaiting an answer.
    pub(crate) fn clear(&mut self) {
        self.items.clear();
    }

    /// How many items are tracked. Read by the adapter's
    /// [`CodexAppServerAdapter::tracked_file_change_items`] observability
    /// accessor, which is what lets a test assert the map really is emptied.
    ///
    /// [`CodexAppServerAdapter::tracked_file_change_items`]: crate::CodexAppServerAdapter::tracked_file_change_items
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delta_usecase::AgentFileChangeKind;

    fn change(path: &str, diff: &str) -> AgentFileChange {
        AgentFileChange {
            path: path.to_owned(),
            kind: Some(AgentFileChangeKind::Update),
            diff: diff.to_owned(),
        }
    }

    #[test]
    fn a_recorded_item_is_readable_by_its_id() {
        let mut items = FileChangeItems::default();
        items.record("item-1".to_owned(), vec![change("a.rs", "-old\n+new")]);

        assert_eq!(
            items.get("item-1"),
            Some(vec![change("a.rs", "-old\n+new")])
        );
        assert_eq!(items.get("item-2"), None, "an unknown item is not invented");
    }

    #[test]
    fn recording_the_same_item_again_replaces_its_changes() {
        let mut items = FileChangeItems::default();
        items.record("item-1".to_owned(), vec![change("a.rs", "first")]);
        items.record("item-1".to_owned(), vec![change("a.rs", "revised")]);

        assert_eq!(
            items.get("item-1"),
            Some(vec![change("a.rs", "revised")]),
            "a revised patch replaces the one item/started announced"
        );
        assert_eq!(items.len(), 1, "the replacement is not a second entry");
    }

    #[test]
    fn forgetting_one_item_leaves_the_others() {
        let mut items = FileChangeItems::default();
        items.record("item-1".to_owned(), vec![change("a.rs", "a")]);
        items.record("item-2".to_owned(), vec![change("b.rs", "b")]);

        items.forget("item-1");

        assert_eq!(items.get("item-1"), None);
        assert_eq!(items.get("item-2"), Some(vec![change("b.rs", "b")]));
    }

    #[test]
    fn clearing_empties_every_entry() {
        let mut items = FileChangeItems::default();
        items.record("item-1".to_owned(), vec![change("a.rs", "a")]);
        items.record("item-2".to_owned(), vec![change("b.rs", "b")]);

        items.clear();

        assert_eq!(items.len(), 0);
    }
}
