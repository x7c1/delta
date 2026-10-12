use delta_model::{SessionId, Thread};

use super::listed_ids;
use crate::interactor::testing::*;
use crate::ports::SessionStore;

/// Every listed session carries its own threads — trunk and branches, in the
/// order `list_threads` returns them — and its trunk id, and a page reads them
/// all with one batch store call: no `list_threads` and no `main_thread_id` per
/// row. A later page carries its own sessions' threads the same way.
#[tokio::test]
async fn list_sessions_page_reads_every_rows_threads_in_one_query() {
    let ix = interactor();

    for id in ["sess-a", "sess-b", "sess-c"] {
        ix.on_user_prompt_submit(submit_for(id, &format!("/tmp/{id}.jsonl"), "seed"))
            .await
            .unwrap();
    }
    // Branch two of the sessions, one of them twice and one branch off a
    // branch, so the rows carry trees of different shapes.
    let store = ix.store();
    let a = SessionId::from("sess-a");
    let c = SessionId::from("sess-c");
    let a_main = store.main_thread_id(&a).await.unwrap();
    let a_branch = store
        .create_thread(&a, "a branch", Some(a_main))
        .await
        .unwrap();
    store
        .create_thread(&a, "a nested branch", Some(a_branch.id))
        .await
        .unwrap();
    let c_main = store.main_thread_id(&c).await.unwrap();
    store
        .create_thread(&c, "c branch", Some(c_main))
        .await
        .unwrap();

    let mut expected = std::collections::HashMap::new();
    for id in ["sess-a", "sess-b", "sess-c"] {
        let id = SessionId::from(id);
        expected.insert(id.clone(), store.list_threads(&id).await.unwrap());
    }
    let reset = || store.inner.lock().unwrap().calls = Default::default();
    let calls = || store.inner.lock().unwrap().calls;

    reset();
    let first = ix.list_sessions_page(None, 2).await.unwrap();
    assert_eq!(listed_ids(&first), vec!["sess-c", "sess-b"]);
    assert_eq!(
        calls().list_threads_by_session_ids,
        1,
        "one batch read per page"
    );
    assert_eq!(calls().list_threads, 0, "no per-row thread read");
    assert_eq!(calls().main_thread_id, 0, "no per-row trunk lookup");

    reset();
    let second = ix.list_sessions_page(first.next, 2).await.unwrap();
    assert_eq!(listed_ids(&second), vec!["sess-a"]);
    assert_eq!(calls().list_threads_by_session_ids, 1);
    assert_eq!(calls().list_threads, 0);
    assert_eq!(calls().main_thread_id, 0);

    for listing in first.listings.iter().chain(&second.listings) {
        let threads = &expected[&listing.session.id];
        assert_eq!(&listing.threads, threads, "{}", listing.session.id.as_str());
        assert_eq!(
            Some(listing.main_thread_id),
            Thread::trunk_of(threads).map(|t| t.id),
            "the trunk id comes from the session's own threads"
        );
    }
    assert_eq!(second.listings[0].threads.len(), 3, "sess-a's whole tree");
}
