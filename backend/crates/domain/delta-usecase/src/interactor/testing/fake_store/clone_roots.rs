//! Registered clone roots.

use crate::error::{Error, Result};
use crate::ports::CloneRoot;

use super::FakeStore;

impl FakeStore {
    pub(super) async fn list_clone_roots(&self) -> Result<Vec<CloneRoot>> {
        let g = self.inner.lock().unwrap();
        // Newest first (descending created_at), mirroring the SQL store. Ties
        // on the seeded timestamp fall back to path ASC for a deterministic order.
        let mut out = g.clone_roots.clone();
        out.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.path.cmp(&b.path))
        });
        Ok(out)
    }

    pub(super) async fn insert_clone_root(&self, path: &str) -> Result<CloneRoot> {
        let mut g = self.inner.lock().unwrap();
        if g.clone_roots.iter().any(|r| r.path == path) {
            return Err(Error::CloneRootDuplicate(path.to_owned()));
        }
        let row = CloneRoot {
            path: path.to_owned(),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        g.clone_roots.push(row.clone());
        Ok(row)
    }

    pub(super) async fn delete_clone_root(&self, path: &str) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.clone_roots.retain(|r| r.path != path);
        Ok(())
    }
}
