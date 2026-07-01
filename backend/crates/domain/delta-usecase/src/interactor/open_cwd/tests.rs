use std::sync::Arc;

use delta_model::SessionId;

use crate::error::Error;
use crate::interactor::open_cwd::VSCODE_HANDLER_ID;
use crate::interactor::testing::*;
use crate::ports::{ExternalOpener, NewSession};

/// Register `/projects/known` as a session cwd so the allowlist accepts it.
async fn seed_known_cwd(
    ix: &crate::Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace, FakeGitWorktree>,
) {
    ix.store()
        .register_session(NewSession {
            id: SessionId::from("s-1"),
            cwd: "/projects/known".into(),
            transcript_path: "/tmp/a.jsonl".into(),
            branch_at_launch: None,
            repo_root: None,
            repository_display_name: None,
        })
        .await
        .unwrap();
}

/// A known cwd + the default handler spawns `code <path>` and reports OK.
#[tokio::test]
async fn open_cwd_default_handler_spawns_code() {
    let opener = Arc::new(FakeExternalOpener::default());
    let ix = interactor().with_external_opener(Arc::clone(&opener) as Arc<dyn ExternalOpener>);
    seed_known_cwd(&ix).await;

    ix.open_cwd("/projects/known", None).await.unwrap();

    let calls = opener.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].command, "code");
    assert_eq!(calls[0].args, vec!["/projects/known".to_string()]);
}

/// An explicit `vscode` handler id resolves to the same command.
#[tokio::test]
async fn open_cwd_explicit_vscode_handler_matches_default() {
    let opener = Arc::new(FakeExternalOpener::default());
    let ix = interactor().with_external_opener(Arc::clone(&opener) as Arc<dyn ExternalOpener>);
    seed_known_cwd(&ix).await;

    ix.open_cwd("/projects/known", Some(VSCODE_HANDLER_ID))
        .await
        .unwrap();

    assert_eq!(opener.calls().len(), 1);
    assert_eq!(opener.calls()[0].command, "code");
}

/// A path outside the allowlist is rejected with `OpenCwdPathNotAllowed`, and
/// the opener is never invoked.
#[tokio::test]
async fn open_cwd_rejects_a_path_not_in_the_allowlist() {
    let opener = Arc::new(FakeExternalOpener::default());
    let ix = interactor().with_external_opener(Arc::clone(&opener) as Arc<dyn ExternalOpener>);

    let err = ix
        .open_cwd("/etc/passwd", None)
        .await
        .expect_err("no session has that cwd, so the reject fires");

    match err {
        Error::OpenCwdPathNotAllowed(path) => assert_eq!(path, "/etc/passwd"),
        other => panic!("expected OpenCwdPathNotAllowed, got {other:?}"),
    }
    assert!(
        opener.calls().is_empty(),
        "a rejected path must never reach the opener"
    );
}

/// An unknown handler id is rejected before the allowlist even checks.
#[tokio::test]
async fn open_cwd_rejects_an_unknown_handler_id() {
    let opener = Arc::new(FakeExternalOpener::default());
    let ix = interactor().with_external_opener(Arc::clone(&opener) as Arc<dyn ExternalOpener>);
    seed_known_cwd(&ix).await;

    let err = ix
        .open_cwd("/projects/known", Some("emacs"))
        .await
        .expect_err("emacs is not registered");

    match err {
        Error::OpenCwdUnknownHandler(id) => assert_eq!(id, "emacs"),
        other => panic!("expected OpenCwdUnknownHandler, got {other:?}"),
    }
    assert!(opener.calls().is_empty());
}

/// A `code`-not-found failure from the opener propagates unchanged.
#[tokio::test]
async fn open_cwd_surfaces_command_not_found_from_the_opener() {
    let opener = Arc::new(FakeExternalOpener::failing_with(
        Error::ExternalOpenerCommandNotFound("code: not on PATH".to_owned()),
    ));
    let ix = interactor().with_external_opener(Arc::clone(&opener) as Arc<dyn ExternalOpener>);
    seed_known_cwd(&ix).await;

    let err = ix
        .open_cwd("/projects/known", None)
        .await
        .expect_err("the opener is scripted to fail");

    assert!(
        matches!(err, Error::ExternalOpenerCommandNotFound(_)),
        "expected ExternalOpenerCommandNotFound, got {err:?}"
    );
}
