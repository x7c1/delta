//! Test helpers shared by the [`StorageInventory`] method modules.

use std::path::Path;

use super::StorageInventory;

/// An inventory writing under `data_dir`, with fixed identifier, port,
/// worktree base (`/w`) and transcript root (`/t`).
pub(super) fn inventory(data_dir: &Path) -> StorageInventory {
    StorageInventory::new("delta-test", data_dir, 7878, "/w", "/t")
}
