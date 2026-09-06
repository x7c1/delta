//! Shared builders for the content fold's tests: the request every source is
//! constructed from, and the plain source most tests start with.

use delta_model::{SessionId, ThreadId};
use delta_usecase::ContentSourceRequest;

use super::CodexConversationSource;

/// The launch directory every source built by [`request`] runs in.
pub(super) const TEST_CWD: &str = "/work/app";

/// A content-source request for `session`, landing on `main_thread` and
/// minting from `seed_seq`, launched in [`TEST_CWD`] with no branch observed
/// there — the shape a session outside a git working tree gets.
pub(super) fn request(session: &str, main_thread: ThreadId, seed_seq: i64) -> ContentSourceRequest {
    ContentSourceRequest {
        session_id: SessionId::from(session),
        main_thread,
        seed_seq,
        cwd: TEST_CWD.to_owned(),
        git_branch: None,
    }
}

/// A source with no model reported and no branch observed, so only the
/// launch directory is stamped.
pub(super) fn source() -> CodexConversationSource {
    CodexConversationSource::new(request("sess-1", ThreadId(1), 0), None)
}
