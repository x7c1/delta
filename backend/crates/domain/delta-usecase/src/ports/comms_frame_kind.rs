//! What kind of message one recorded agent frame is.

#[cfg(doc)]
use super::CommsDirection;

/// What kind of message one recorded frame is.
///
/// Deliberately three variants, not four: a *server-originated request* is not
/// a different kind of message from a request — it is a request travelling the
/// other way. Every frame shape a JSON-RPC-style transport carries is therefore
/// a (direction, kind) pair:
///
/// | frame | direction | kind |
/// |---|---|---|
/// | client request | [`CommsDirection::ToAgent`] | [`CommsFrameKind::Request`] |
/// | client notification | `ToAgent` | [`CommsFrameKind::Notification`] |
/// | client response (answering a server request) | `ToAgent` | [`CommsFrameKind::Response`] |
/// | server response | [`CommsDirection::FromAgent`] | `Response` |
/// | server request | `FromAgent` | `Request` |
/// | server notification | `FromAgent` | `Notification` |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommsFrameKind {
    /// Carries an id and awaits a correlated response.
    Request,
    /// Answers a request, carrying its id.
    Response,
    /// Fire and forget: no id, no answer.
    Notification,
}
