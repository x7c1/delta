use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::NewSession;

/// `recent_workdirs` surfaces the distinct cwds from the store, most-recent
/// first.
#[tokio::test]
async fn recent_workdirs_lists_distinct_session_cwds() {
    let ix = interactor();
    // Register two sessions with distinct cwds via the store directly.
    ix.store()
        .register_session(NewSession {
            id: SessionId::from("s-1"),
            cwd: "/projects/a".into(),
            transcript_path: "/tmp/a.jsonl".into(),
            branch_at_launch: None,
            repo_root: None,
        })
        .await
        .unwrap();
    ix.store()
        .register_session(NewSession {
            id: SessionId::from("s-2"),
            cwd: "/projects/b".into(),
            transcript_path: "/tmp/b.jsonl".into(),
            branch_at_launch: None,
            repo_root: None,
        })
        .await
        .unwrap();

    let recent = ix.recent_workdirs().await.unwrap();
    let mut paths: Vec<&str> = recent.iter().map(|(p, _)| p.as_str()).collect();
    paths.sort();
    assert_eq!(paths, vec!["/projects/a", "/projects/b"]);
}
