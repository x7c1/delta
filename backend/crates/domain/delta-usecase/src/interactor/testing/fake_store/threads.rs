//! Thread rows, with the root message and last activity derived on read.

use delta_model::{MessageUuid, Role, SessionId, Thread, ThreadId};

use crate::error::Result;

use super::{FakeStore, FakeStoreInner};

/// Derive a thread's `root_message_uuid` the way the SQL store does: the
/// `semantic_parent_uuid` of the thread's first semantically parented message,
/// falling back to its earliest semantically parented send.
fn derive_root_message_uuid(g: &FakeStoreInner, thread_id: ThreadId) -> Option<MessageUuid> {
    g.messages
        .iter()
        .filter(|m| m.thread_id == thread_id && m.semantic_parent_uuid.is_some())
        .min_by_key(|m| m.seq)
        .and_then(|m| m.semantic_parent_uuid.clone())
        .or_else(|| {
            g.sends
                .iter()
                .filter(|s| s.thread_id == thread_id && s.semantic_parent_uuid.is_some())
                .min_by_key(|s| s.id)
                .and_then(|s| s.semantic_parent_uuid.clone())
        })
}

/// Derive a thread's `last_activity_at` the way the SQL store maintains it:
/// `MAX(message.created_at)` over the thread's own messages, `None` when none of
/// them carries a timestamp. The real store denormalizes this onto the thread
/// row on every upsert; the fake recomputes it on read, which by construction is
/// the same value.
fn derive_last_activity_at(g: &FakeStoreInner, thread_id: ThreadId) -> Option<String> {
    g.messages
        .iter()
        .filter(|m| m.thread_id == thread_id)
        .filter_map(|m| m.created_at.clone())
        .max()
}

impl FakeStore {
    pub(super) async fn main_thread_id(&self, session_id: &SessionId) -> Result<ThreadId> {
        let g = self.inner.lock().unwrap();
        Ok(g.threads
            .iter()
            .find(|t| &t.session_id == session_id && t.title == "main")
            .unwrap()
            .id)
    }

    pub(super) async fn thread(&self, id: ThreadId) -> Result<Option<Thread>> {
        let g = self.inner.lock().unwrap();
        Ok(g.threads.iter().find(|t| t.id == id).cloned().map(|mut t| {
            t.root_message_uuid = derive_root_message_uuid(&g, t.id);
            t.last_activity_at = derive_last_activity_at(&g, t.id);
            t
        }))
    }

    pub(super) async fn list_threads(&self, session_id: &SessionId) -> Result<Vec<Thread>> {
        let g = self.inner.lock().unwrap();
        let mut out: Vec<Thread> = g
            .threads
            .iter()
            .filter(|t| &t.session_id == session_id)
            .cloned()
            .map(|mut t| {
                t.root_message_uuid = derive_root_message_uuid(&g, t.id);
                t.last_activity_at = derive_last_activity_at(&g, t.id);
                t
            })
            .collect();
        out.sort_by_key(|t| t.id);
        Ok(out)
    }

    pub(super) async fn create_thread(
        &self,
        session_id: &SessionId,
        title: &str,
        parent_thread_id: Option<ThreadId>,
    ) -> Result<Thread> {
        let mut g = self.inner.lock().unwrap();
        g.next_thread_id += 1;
        let thread = Thread {
            id: ThreadId(g.next_thread_id),
            session_id: session_id.clone(),
            title: title.to_owned(),
            parent_thread_id,
            // Derived on read (from the thread's branch send/message), mirroring
            // the real store; see `derive_root_message_uuid`.
            root_message_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            // Likewise derived on read; a thread with no messages has none.
            last_activity_at: None,
        };
        g.threads.push(thread.clone());
        Ok(thread)
    }

    pub(super) async fn latest_user_thread(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<ThreadId>> {
        let g = self.inner.lock().unwrap();
        Ok(g.messages
            .iter()
            .filter(|m| &m.session_id == session_id && matches!(m.role, Role::User))
            .max_by_key(|m| m.seq)
            .map(|m| m.thread_id))
    }
}
