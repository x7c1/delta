//! Tests for the repository-clone use case.
//!
//! These run against a real temporary directory (the module does its own
//! filesystem work, exactly as the clone-root scan does) plus the `gh` fake, so
//! every assertion is about what actually ends up on disk and what the browser
//! is told.

use std::sync::Arc;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::Error;

/// How long a test waits for a spawned clone job to reach a checkpoint before
/// declaring the behaviour broken. Generous, because it only ever elapses on
/// failure — the happy paths resolve in microseconds.
const WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Await one event from the seam, failing the test rather than hanging forever
/// if the job never reports.
async fn next_event(rx: &mut crate::ports::AsyncEventReceiver) -> SessionEvent {
    tokio::time::timeout(WAIT, rx.recv())
        .await
        .expect("the clone job reports its outcome")
        .expect("the event seam stays open")
}

/// Spin until `predicate` holds, so a test can wait for a spawned job to reach
/// the gh call without sleeping a fixed duration.
async fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + WAIT;
    while !predicate() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for the clone job to make progress",
        );
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn an_unregistered_clone_root_is_refused_and_starts_no_job() {
    let gh = Arc::new(FakeGhCli::cloning());
    let (ix, _rx) = interactor_with_gh_and_event_sink(Arc::clone(&gh));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap();
    // Deliberately NOT registered: Delta writes clones only where the user said
    // clones go, so an arbitrary directory is refused even though it exists.

    let err = ix
        .clone_repository("x7c1", "delta", root)
        .await
        .expect_err("an unregistered clone root is refused");

    assert!(
        matches!(&err, Error::CloneRootNotRegistered(path) if path == root),
        "got: {err:?}",
    );
    assert!(
        gh.clone_calls().is_empty(),
        "no gh process may start for a refused request",
    );
}

#[tokio::test]
async fn an_existing_destination_is_refused_and_starts_no_job() {
    let gh = Arc::new(FakeGhCli::cloning());
    let (ix, _rx) = interactor_with_gh_and_event_sink(Arc::clone(&gh));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap().to_owned();
    ix.store().insert_clone_root(&root).await.unwrap();
    // Something already occupies `<root>/delta`. There is no fallback naming,
    // so the request is refused rather than landing next to it.
    std::fs::create_dir(tmp.path().join("delta")).unwrap();

    let err = ix
        .clone_repository("x7c1", "delta", &root)
        .await
        .expect_err("an occupied destination is refused");

    assert!(
        matches!(&err, Error::CloneDestinationExists(path) if path == &format!("{root}/delta")),
        "got: {err:?}",
    );
    assert!(
        gh.clone_calls().is_empty(),
        "no gh process may start for a refused request",
    );
}

#[tokio::test]
async fn a_repo_name_that_escapes_the_root_is_refused() {
    // The destination is built by joining the name onto the clone root, so a
    // name carrying a separator or a parent segment would write outside the root
    // the request named.
    let gh = Arc::new(FakeGhCli::cloning());
    let (ix, _rx) = interactor_with_gh_and_event_sink(Arc::clone(&gh));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap().to_owned();
    ix.store().insert_clone_root(&root).await.unwrap();

    for name in ["..", "../escape", "nested/name", ""] {
        let err = ix
            .clone_repository("x7c1", name, &root)
            .await
            .expect_err("a name that is not one path component is refused");
        assert!(
            matches!(&err, Error::InvalidRepositoryRef(_)),
            "{name:?} got: {err:?}",
        );
    }
    assert!(gh.clone_calls().is_empty(), "no gh process may start");
}

#[tokio::test]
async fn clone_repository_rejects_an_owner_beginning_with_a_dash() {
    // The slug reaches `gh repo clone` as a positional argument, so an owner or
    // name that begins with `-` (e.g. `-x`, `--upload-pack=…`) could be parsed
    // as a flag. `check_path_segment` refuses it before any `gh` process starts.
    let gh = Arc::new(FakeGhCli::cloning());
    let (ix, _rx) = interactor_with_gh_and_event_sink(Arc::clone(&gh));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap().to_owned();
    ix.store().insert_clone_root(&root).await.unwrap();

    // A dash-leading owner is refused...
    let err = ix
        .clone_repository("-x7c1", "delta", &root)
        .await
        .expect_err("a dash-leading owner is refused");
    assert!(
        matches!(&err, Error::InvalidRepositoryRef(_)),
        "got: {err:?}"
    );
    // ...and so is a dash-leading repository name.
    let err = ix
        .clone_repository("x7c1", "--flag", &root)
        .await
        .expect_err("a dash-leading name is refused");
    assert!(
        matches!(&err, Error::InvalidRepositoryRef(_)),
        "got: {err:?}"
    );

    assert!(gh.clone_calls().is_empty(), "no gh process may start");
}

#[tokio::test]
async fn the_happy_path_clones_into_a_temp_dir_renames_it_and_announces_completion() {
    let gh = Arc::new(FakeGhCli::cloning());
    let (ix, mut rx) = interactor_with_gh_and_event_sink(Arc::clone(&gh));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap().to_owned();
    ix.store().insert_clone_root(&root).await.unwrap();

    ix.clone_repository("x7c1", "delta", &root).await.unwrap();

    let event = next_event(&mut rx).await;
    let destination = format!("{root}/delta");
    assert_eq!(
        event,
        SessionEvent::RepositoryCloneCompleted {
            repo_owner: "x7c1".into(),
            repo_name: "delta".into(),
            clone_root: root.clone(),
            destination_path: destination.clone(),
        },
    );

    // gh was pointed at the temporary sibling, never at the destination: that
    // is what keeps a half-written clone off the path the browser watches.
    let calls = gh.clone_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].owner, "x7c1");
    assert_eq!(calls[0].name, "delta");
    assert_eq!(
        calls[0].destination,
        format!("{root}/.delta-clone-tmp-delta")
    );

    // …and the finished tree was renamed onto the destination, leaving no
    // temporary directory behind.
    assert!(
        tmp.path().join("delta").join(CLONE_MARKER).exists(),
        "the cloned tree is at the destination",
    );
    assert!(
        !tmp.path().join(".delta-clone-tmp-delta").exists(),
        "the temporary directory is gone once it has been renamed",
    );
}

#[tokio::test]
async fn a_failing_clone_removes_the_temp_dir_and_announces_the_message() {
    let gh = Arc::new(FakeGhCli::failing_clone(
        "could not resolve host github.com",
    ));
    let (ix, mut rx) = interactor_with_gh_and_event_sink(Arc::clone(&gh));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap().to_owned();
    ix.store().insert_clone_root(&root).await.unwrap();

    ix.clone_repository("x7c1", "delta", &root).await.unwrap();

    let event = next_event(&mut rx).await;
    let SessionEvent::RepositoryCloneFailed {
        repo_owner,
        repo_name,
        clone_root,
        destination_path,
        message,
    } = event
    else {
        panic!("a failing clone announces a failure, got: {event:?}");
    };
    assert_eq!(repo_owner, "x7c1");
    assert_eq!(repo_name, "delta");
    assert_eq!(clone_root, root);
    assert_eq!(destination_path, format!("{root}/delta"));
    assert!(
        message.contains("could not resolve host github.com"),
        "the failure carries gh's own words: {message}",
    );

    assert!(
        !tmp.path().join(".delta-clone-tmp-delta").exists(),
        "a failed clone leaves no temporary directory behind",
    );
    assert!(
        !tmp.path().join("delta").exists(),
        "a failed clone never creates the destination",
    );
}

#[tokio::test]
async fn a_retry_after_a_failure_starts_a_new_job_instead_of_joining_the_dead_one() {
    // "Retrying is simply the same request again" is what the API doc, the
    // failure event and the browser's inline error all promise, and it holds
    // only because the failed job retires its claim *before* announcing. A retry
    // that joined the dead job would be accepted and then wait forever for an
    // event that has already been sent.
    let gh = Arc::new(FakeGhCli::failing_clone(
        "could not resolve host github.com",
    ));
    let (ix, mut rx) = interactor_with_gh_and_event_sink(Arc::clone(&gh));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap().to_owned();
    ix.store().insert_clone_root(&root).await.unwrap();

    ix.clone_repository("x7c1", "delta", &root).await.unwrap();
    let first = next_event(&mut rx).await;
    assert!(
        matches!(first, SessionEvent::RepositoryCloneFailed { .. }),
        "got: {first:?}",
    );

    // The user clicks Clone again, on the same row and the same root.
    ix.clone_repository("x7c1", "delta", &root).await.unwrap();
    let second = next_event(&mut rx).await;
    assert!(
        matches!(second, SessionEvent::RepositoryCloneFailed { .. }),
        "the retry reports its own outcome, got: {second:?}",
    );
    assert_eq!(
        gh.clone_calls().len(),
        2,
        "the retry runs its own gh process rather than joining the failed job",
    );
}

#[tokio::test]
async fn a_second_request_for_the_same_destination_joins_the_running_job() {
    let gh = Arc::new(FakeGhCli::blocking_clone());
    let (ix, mut rx) = interactor_with_gh_and_event_sink(Arc::clone(&gh));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap().to_owned();
    ix.store().insert_clone_root(&root).await.unwrap();

    ix.clone_repository("x7c1", "delta", &root).await.unwrap();
    // Wait until the job is genuinely parked inside gh, so the second request
    // races a job that is unambiguously in flight.
    wait_until(|| gh.clone_calls().len() == 1).await;

    // The double-click: accepted, but it must not start a second `gh`.
    ix.clone_repository("x7c1", "delta", &root).await.unwrap();

    gh.release_clone();
    let event = next_event(&mut rx).await;
    assert!(matches!(
        event,
        SessionEvent::RepositoryCloneCompleted { .. }
    ));
    assert_eq!(
        gh.clone_calls().len(),
        1,
        "the joined request reuses the running job's gh process",
    );
    assert!(
        rx.try_recv().is_err(),
        "one completion serves both requests",
    );
}

#[tokio::test]
async fn jobs_for_different_repositories_run_concurrently() {
    let gh = Arc::new(FakeGhCli::blocking_clone());
    let (ix, mut rx) = interactor_with_gh_and_event_sink(Arc::clone(&gh));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap().to_owned();
    ix.store().insert_clone_root(&root).await.unwrap();

    ix.clone_repository("x7c1", "delta", &root).await.unwrap();
    ix.clone_repository("x7c1", "other", &root).await.unwrap();

    // Both are inside gh at the same time — the registry is keyed by
    // destination, so distinct repositories never serialise behind each other.
    wait_until(|| gh.clone_calls().len() == 2).await;

    gh.release_clone();
    let mut cloned = vec![
        clone_destination(next_event(&mut rx).await),
        clone_destination(next_event(&mut rx).await),
    ];
    cloned.sort();
    assert_eq!(
        cloned,
        vec![format!("{root}/delta"), format!("{root}/other")]
    );
}

/// The destination path of a completion event, failing the test for any other
/// event.
fn clone_destination(event: SessionEvent) -> String {
    match event {
        SessionEvent::RepositoryCloneCompleted {
            destination_path, ..
        } => destination_path,
        other => panic!("expected a completed clone, got: {other:?}"),
    }
}

#[tokio::test]
async fn a_stale_temp_dir_from_a_dead_job_is_removed_before_the_new_one_starts() {
    // What a server killed mid-clone leaves behind: a temporary directory with a
    // partial clone in it and no job registry to remember it (the registry died
    // with the process). The next job for that destination must clear it —
    // otherwise `gh` refuses the non-empty target, or worse, the leftovers get
    // renamed onto the destination as if they were a finished clone.
    let gh = Arc::new(FakeGhCli::cloning());
    let (ix, mut rx) = interactor_with_gh_and_event_sink(Arc::clone(&gh));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap().to_owned();
    ix.store().insert_clone_root(&root).await.unwrap();

    let stale = tmp.path().join(".delta-clone-tmp-delta");
    std::fs::create_dir(&stale).unwrap();
    std::fs::write(stale.join("half-written"), b"from a dead job\n").unwrap();

    ix.clone_repository("x7c1", "delta", &root).await.unwrap();
    let event = next_event(&mut rx).await;
    assert!(matches!(
        event,
        SessionEvent::RepositoryCloneCompleted { .. }
    ));

    let destination = tmp.path().join("delta");
    assert!(
        destination.join(CLONE_MARKER).exists(),
        "the destination holds the new clone",
    );
    assert!(
        !destination.join("half-written").exists(),
        "nothing from the dead job survives into the destination",
    );
}

#[tokio::test]
async fn a_session_can_be_started_while_a_clone_is_in_flight() {
    // The clone registry is not on any session path. A job parked inside `gh`
    // holds its own lock and nothing else, so spawning a session neither waits
    // for it nor is waited on.
    let gh = Arc::new(FakeGhCli::blocking_clone());
    let (ix, _rx) = interactor_with_gh_and_event_sink(Arc::clone(&gh));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_str().unwrap().to_owned();
    ix.store().insert_clone_root(&root).await.unwrap();

    ix.clone_repository("x7c1", "delta", &root).await.unwrap();
    wait_until(|| gh.clone_calls().len() == 1).await;

    let lifecycle = tokio::time::timeout(WAIT, ix.ensure_session())
        .await
        .expect("starting a session does not wait on the clone registry")
        .expect("the session spawns");
    assert_eq!(
        lifecycle,
        crate::ports::SessionLifecycle::Starting,
        "a real session was spawned while the clone was still running",
    );

    // And the clone is still in flight — the session spawn did not disturb it.
    assert_eq!(gh.clone_calls().len(), 1);
}
