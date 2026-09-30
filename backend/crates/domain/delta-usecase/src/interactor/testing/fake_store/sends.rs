//! Send rows: the per-session queue, dispatch, hold/release, and settling.

use delta_model::{MessageUuid, Send, SendStatus, SessionId, ThreadId};

use crate::error::{Error, Result};

use super::{FakeStore, HELD_AT};

impl FakeStore {
    pub(super) async fn enqueue_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
        let mut g = self.inner.lock().unwrap();
        g.next_send_id += 1;
        let send = Send {
            id: g.next_send_id,
            session_id: session_id.clone(),
            thread_id,
            semantic_parent_uuid: semantic_parent_uuid.cloned(),
            text: text.to_owned(),
            locator_quote: locator_quote.map(str::to_owned),
            status: SendStatus::Dispatched,
            matched_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            held_at: None,
        };
        g.sends.push(send.clone());
        Ok(send)
    }

    pub(super) async fn enqueue_queued_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
        let mut g = self.inner.lock().unwrap();
        g.next_send_id += 1;
        let send = Send {
            id: g.next_send_id,
            session_id: session_id.clone(),
            thread_id,
            semantic_parent_uuid: semantic_parent_uuid.cloned(),
            text: text.to_owned(),
            locator_quote: locator_quote.map(str::to_owned),
            status: SendStatus::Queued,
            matched_uuid: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            held_at: None,
        };
        g.sends.push(send.clone());
        Ok(send)
    }

    pub(super) async fn send(&self, id: i64) -> Result<Option<Send>> {
        let g = self.inner.lock().unwrap();
        Ok(g.sends.iter().find(|s| s.id == id).cloned())
    }

    pub(super) async fn next_queued_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        let g = self.inner.lock().unwrap();
        Ok(g.sends
            .iter()
            .filter(|s| {
                &s.session_id == session_id
                    && s.status == SendStatus::Queued
                    // Held rows never dispatch automatically; they wait
                    // for an explicit release, mirroring the SQL filter.
                    && s.held_at.is_none()
            })
            .min_by_key(|s| s.id)
            .cloned())
    }

    pub(super) async fn open_sends(&self, session_id: &SessionId) -> Result<Vec<Send>> {
        let g = self.inner.lock().unwrap();
        let mut out: Vec<Send> = g
            .sends
            .iter()
            .filter(|s| {
                &s.session_id == session_id
                    && matches!(s.status, SendStatus::Queued | SendStatus::Dispatched)
            })
            .cloned()
            .collect();
        out.sort_by_key(|s| s.id);
        Ok(out)
    }

    pub(super) async fn promote_queued_send(&self, id: i64) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g.sends.iter_mut().find(|s| s.id == id) {
            s.status = SendStatus::Dispatched;
        }
        Ok(())
    }

    pub(super) async fn requeue_send(&self, id: i64) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g
            .sends
            .iter_mut()
            .find(|s| s.id == id && s.status == SendStatus::Dispatched)
        {
            s.status = SendStatus::Queued;
        }
        Ok(())
    }

    pub(super) async fn restore_all_dispatched(&self) -> Result<usize> {
        let mut g = self.inner.lock().unwrap();
        let mut restored = 0;
        for s in g
            .sends
            .iter_mut()
            .filter(|s| s.status == SendStatus::Dispatched)
        {
            s.status = SendStatus::Queued;
            s.held_at = Some(HELD_AT.into());
            restored += 1;
        }
        Ok(restored)
    }

    pub(super) async fn hold_send_for_release(&self, id: i64) -> Result<bool> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g
            .sends
            .iter_mut()
            .find(|s| s.id == id && s.status == SendStatus::Dispatched)
        {
            s.status = SendStatus::Queued;
            s.held_at = Some(HELD_AT.into());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub(super) async fn release_held_send(&self, id: i64) -> Result<bool> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g
            .sends
            .iter_mut()
            .find(|s| s.id == id && s.status == SendStatus::Queued && s.held_at.is_some())
        {
            s.held_at = None;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub(super) async fn head_dispatched_send(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<Send>> {
        let g = self.inner.lock().unwrap();
        Ok(g.sends
            .iter()
            .filter(|s| &s.session_id == session_id && s.status == SendStatus::Dispatched)
            .min_by_key(|s| s.id)
            .cloned())
    }

    pub(super) async fn dispatched_sends(&self, session_id: &SessionId) -> Result<Vec<Send>> {
        let g = self.inner.lock().unwrap();
        let mut out: Vec<Send> = g
            .sends
            .iter()
            .filter(|s| &s.session_id == session_id && s.status == SendStatus::Dispatched)
            .cloned()
            .collect();
        out.sort_by_key(|s| s.id);
        Ok(out)
    }

    pub(super) async fn mark_send_matched(
        &self,
        id: i64,
        matched_uuid: &MessageUuid,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g.sends.iter_mut().find(|s| s.id == id) {
            s.status = SendStatus::Matched;
            s.matched_uuid = Some(matched_uuid.clone());
        }
        Ok(())
    }

    pub(super) async fn settle_send_delivered(&self, id: i64) -> Result<bool> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g
            .sends
            .iter_mut()
            .find(|s| s.id == id && s.status == SendStatus::Dispatched)
        {
            // Delivered, but no transcript line claimed it: `matched_uuid`
            // stays `None`, exactly as the SQL leaves the column `NULL`.
            s.status = SendStatus::Matched;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub(super) async fn cancel_send(&self, id: i64) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if g.fail_cancel_send {
            return Err(Error::Store("injected cancel_send failure".into()));
        }
        if let Some(s) = g.sends.iter_mut().find(|s| s.id == id) {
            s.status = SendStatus::Cancelled;
        }
        Ok(())
    }

    pub(super) async fn cancel_queued_send(&self, id: i64) -> Result<bool> {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = g
            .sends
            .iter_mut()
            .find(|s| s.id == id && s.status == SendStatus::Queued)
        {
            s.status = SendStatus::Cancelled;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
