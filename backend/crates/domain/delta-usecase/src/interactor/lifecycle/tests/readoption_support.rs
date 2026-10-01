//! Shared setup for the re-adoption tests: a session a previous Delta process
//! left behind, whose row remembers a pane.

use delta_model::SessionId;

use crate::SessionListing;

use crate::interactor::testing::*;
pub(super) use crate::ports::pane_for;
use crate::ports::{NewSession, RememberedPane};

/// The pane every re-adoption test's previous process left behind.
pub(super) const SURVIVING_TOKEN: &str = "delta-7";

/// Register `id` as a closed session whose transcript lives at
/// `/work/<id>.jsonl` and whose row remembers the tmux session `token` — what a
/// session open in the previous process looks like to the next one.
///
/// The transcript holds one line the previous process already ingested (the
/// stored cursor is past it), so a catch-up that re-read from the start would
/// show up as a second message.
pub(super) async fn left_behind(ix: &TestInteractor, id: &str, token: &str) -> SessionId {
    let path = format!("/work/{id}.jsonl");
    let (session, _) = ix
        .store()
        .register_session(NewSession {
            id: id.into(),
            cwd: "/work".into(),
            transcript_path: path.clone(),
            branch_at_launch: None,
            repo_root: None,
            repository_display_name: None,
        })
        .await
        .unwrap();
    ix.transcript_fake()
        .push_to(&path, user_line(&format!("{id}-seen"), "already ingested"));
    ix.store()
        .set_transcript_lines_read(&session.id, 1)
        .await
        .unwrap();
    ix.store()
        .remember_pane(
            &session.id,
            &RememberedPane {
                tmux_session: token.into(),
                pane: pane_for(token),
                hooks_unreachable: false,
            },
        )
        .await
        .unwrap();
    session.id
}

/// Make `token` a tmux session that is still running, as one that survived the
/// restart is.
pub(super) fn survives(ix: &TestInteractor, token: &str) {
    ix.tmux_fake().live.lock().unwrap().push(token.to_owned());
}

/// The session's row in the first page of the session list.
pub(super) async fn listing(ix: &TestInteractor, id: &SessionId) -> SessionListing {
    ix.list_sessions_page(None, 50)
        .await
        .unwrap()
        .listings
        .into_iter()
        .find(|l| &l.session.id == id)
        .expect("listed")
}
