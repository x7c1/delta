use delta_model::SessionId;

use crate::interactor::testing::*;

use super::support::round_trip;

#[tokio::test]
async fn same_thread_continuation_injects_nothing() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Two consecutive plain sends to the same (main) thread. The second is a
    // same-thread continuation, so nothing is injected.
    round_trip(&ix, to(main), "first on main", None, "u-1").await;
    let (_, additional) = round_trip(&ix, to(main), "second on main", None, "u-2").await;
    assert!(additional.is_none());
}
