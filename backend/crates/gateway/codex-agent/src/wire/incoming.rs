//! A parsed incoming frame, classified by which JSON-RPC fields it carries.

use serde_json::Value;

use super::{Notification, Response, ServerRequest, THREAD_ID_KEY};

/// A parsed incoming frame, classified by which JSON-RPC fields it carries.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A response to one of our requests (`id` + `result`/`error`, no `method`).
    Response(Response),
    /// A server-originated request (`id` + `method`).
    ServerRequest(ServerRequest),
    /// A server-originated notification (`method`, no `id`).
    Notification(Notification),
}

impl Incoming {
    /// The thread id this frame is scoped to, if it carries one under
    /// `params.threadId`. Responses are correlated by id rather than thread, so
    /// they never carry one here.
    pub fn thread_id(&self) -> Option<&str> {
        let params = match self {
            Incoming::Notification(n) => &n.params,
            Incoming::ServerRequest(r) => &r.params,
            Incoming::Response(_) => return None,
        };
        params.get(THREAD_ID_KEY).and_then(Value::as_str)
    }
}
