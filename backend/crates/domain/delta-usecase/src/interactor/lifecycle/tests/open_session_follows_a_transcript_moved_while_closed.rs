//! Resuming a session whose transcript Claude Code moved while Delta was not
//! following it.
//!
//! Claude Code moves a session's `<session id>.jsonl` to another project
//! directory when the session enters a worktree. A session that was closed
//! (or whose Delta was down) at the time never sent the hook that reports the
//! new path, so its stored path names a file that no longer exists. Resuming it
//! looks the file up under the transcript root, re-points the session at it,
//! and launches `claude --resume` in the working directory the file's
//! `relocated` line names.

use delta_model::SessionId;

use crate::error::Error;
use crate::interactor::testing::*;

const SESSION: &str = "sess-R";
/// The worktree the `relocated` line names.
const NEW_CWD: &str = "/work/wt";

fn sess() -> SessionId {
    SessionId::from(SESSION)
}

/// A transcript root on disk (path confinement canonicalizes it) and the
/// session's transcript path in two of its project directories.
struct Layout {
    root: tempfile::TempDir,
    old_path: String,
    new_path: String,
}

fn layout() -> Layout {
    let root = tempfile::tempdir().unwrap();
    let path = |project: &str| {
        root.path()
            .join(project)
            .join(format!("{SESSION}.jsonl"))
            .to_str()
            .unwrap()
            .to_owned()
    };
    let old_path = path("-work");
    let new_path = path("-work-wt");
    Layout {
        root,
        old_path,
        new_path,
    }
}

/// Register the session on `old_path` with one ingested exchange (cursor 2),
/// closed — an external `claude` Delta only observed through its hooks.
async fn closed_session_with_one_exchange(layout: &Layout) -> TestInteractor {
    let ix = interactor_with_transcript_root(layout.root.path().to_str().unwrap());
    ix.on_user_prompt_submit(submit_in(SESSION, &layout.old_path, "/work", "seed"))
        .await
        .unwrap();
    ix.transcript_fake()
        .push_to(&layout.old_path, user_line("u-0", "seed"));
    ix.transcript_fake()
        .push_to(&layout.old_path, assistant_line("a-0", "first reply"));
    ix.on_stop(crate::ports::StopHook {
        session_id: sess(),
        stop_reason: None,
    })
    .await
    .unwrap();
    assert_eq!(ix.store().transcript_lines_read(&sess()).await.unwrap(), 2);
    assert!(ix.bound_pane(&sess()).await.is_none(), "starts closed");
    ix
}

#[tokio::test]
async fn resuming_follows_a_transcript_moved_while_the_session_was_closed() {
    let layout = layout();
    let ix = closed_session_with_one_exchange(&layout).await;

    // The session enters a worktree with no hook reaching Delta: the file
    // moves whole, gains a `relocated` line, and then a reply Delta has not
    // seen.
    ix.transcript_fake()
        .relocate(&layout.old_path, &layout.new_path, NEW_CWD);
    ix.transcript_fake().push_to(
        &layout.new_path,
        assistant_line("a-1", "reply from the worktree"),
    );

    ix.open_session(&sess()).await.unwrap();

    let session = ix.store().session(&sess()).await.unwrap().unwrap();
    assert_eq!(
        session.transcript_path.as_deref(),
        Some(layout.new_path.as_str()),
        "the session is re-pointed at the moved transcript"
    );
    assert_eq!(
        session.cwd, NEW_CWD,
        "the session takes the cwd the relocated line names"
    );

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    let resume = created
        .iter()
        .find(|c| c.command.iter().any(|a| a == "--resume"))
        .expect("a resume spawn was recorded");
    assert_eq!(
        resume.workdir, NEW_CWD,
        "claude --resume runs where Claude Code now files the transcript"
    );
    assert!(ix.bound_pane(&sess()).await.is_some(), "now open");

    // The resume's catch-up sync ingests the post-move reply from the moved
    // file, continuing at the kept cursor: nothing is read twice.
    assert_eq!(ix.store().message_count(&sess()).await.unwrap(), 3);
    // Two old lines, the `relocated` line, and the new reply.
    assert_eq!(ix.store().transcript_lines_read(&sess()).await.unwrap(), 4);
}

#[tokio::test]
async fn resuming_with_no_moved_transcript_found_is_still_refused() {
    let layout = layout();
    let ix = closed_session_with_one_exchange(&layout).await;

    // The transcript is gone and no project directory holds it.
    ix.transcript_fake().mark_missing(&layout.old_path);

    let err = ix
        .open_session(&sess())
        .await
        .expect_err("with no transcript anywhere, resume is impossible");
    assert!(
        matches!(err, Error::ResumeUnavailable(ref s) if s == SESSION),
        "got: {err:?}"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a refused resume spawns no pane"
    );
    assert!(ix.bound_pane(&sess()).await.is_none(), "stays closed");
    let session = ix.store().session(&sess()).await.unwrap().unwrap();
    assert_eq!(
        session.transcript_path.as_deref(),
        Some(layout.old_path.as_str()),
        "the stored path is left alone"
    );
}
