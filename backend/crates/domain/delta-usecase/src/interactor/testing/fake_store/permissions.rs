//! Permission-request rows.

use delta_model::{PermissionRequest, PermissionStatus, SessionId};

use crate::error::Result;

use super::FakeStore;

impl FakeStore {
    pub(super) async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: Option<&str>,
    ) -> Result<PermissionRequest> {
        let mut g = self.inner.lock().unwrap();
        g.next_perm_id += 1;
        let req = PermissionRequest {
            id: g.next_perm_id,
            session_id: session_id.clone(),
            tool_name: tool_name.to_owned(),
            tool_input_json: tool_input_json.to_owned(),
            tool_use_id: tool_use_id.map(str::to_owned),
            status: PermissionStatus::Pending,
            decision_reason: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            decided_at: None,
        };
        g.permissions.push(req.clone());
        Ok(req)
    }

    pub(super) async fn decide_permission_request(
        &self,
        request_id: i64,
        allowed: bool,
    ) -> Result<Option<PermissionRequest>> {
        let mut g = self.inner.lock().unwrap();
        let req = g
            .permissions
            .iter_mut()
            .find(|r| r.id == request_id && r.status == PermissionStatus::Pending);
        match req {
            Some(req) => {
                req.status = if allowed {
                    PermissionStatus::Allowed
                } else {
                    PermissionStatus::Denied
                };
                req.decided_at = Some("2026-01-01T00:00:00Z".into());
                Ok(Some(req.clone()))
            }
            None => Ok(None),
        }
    }

    pub(super) async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> Result<Vec<i64>> {
        let mut g = self.inner.lock().unwrap();
        // Mirror the SQL: settle the PreToolUse row matching `tool_use_id` AND
        // any pending dialog row (`tool_use_id: None`) for the same session.
        let mut resolved = Vec::new();
        for req in g.permissions.iter_mut().filter(|r| {
            &r.session_id == session_id
                && r.status == PermissionStatus::Pending
                && (r.tool_use_id.as_deref() == Some(tool_use_id) || r.tool_use_id.is_none())
        }) {
            req.status = if allowed {
                PermissionStatus::Allowed
            } else {
                PermissionStatus::Denied
            };
            req.decided_at = Some("2026-01-01T00:00:00Z".into());
            resolved.push(req.id);
        }
        Ok(resolved)
    }

    pub(super) async fn deny_pending_permission_requests(
        &self,
        session_id: &SessionId,
        reason: &str,
    ) -> Result<Vec<i64>> {
        let mut g = self.inner.lock().unwrap();
        // Mirror the SQL: every still-pending row of the session becomes
        // `denied` carrying the reason, and only the ids that transitioned come
        // back.
        let mut denied = Vec::new();
        for req in g
            .permissions
            .iter_mut()
            .filter(|r| &r.session_id == session_id && r.status == PermissionStatus::Pending)
        {
            req.status = PermissionStatus::Denied;
            req.decision_reason = Some(reason.to_owned());
            req.decided_at = Some("2026-01-01T00:00:00Z".into());
            denied.push(req.id);
        }
        Ok(denied)
    }
}
