use delta_model::SessionId;

use crate::interactor::testing::*;

#[tokio::test]
async fn plain_send_attributes_user_and_assistant_to_main() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (pending, _) = ix
        .enqueue_send(to(main), "hello world", None)
        .await
        .unwrap();
    assert_eq!(pending.thread_id, main);

    // The matching user line is ingested + correlated.
    ix.transcript_fake().push(user_line("u-1", "hello world"));
    ix.on_user_prompt_submit(submit("hello world"))
        .await
        .unwrap();

    // The assistant response arrives and is ingested at Stop.
    ix.transcript_fake().push(assistant_line("a-1", "hi there"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    let view = ix.thread_view(main).await.unwrap();
    let uuids: Vec<&str> = view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(uuids.contains(&"u-1"), "user message lands on main");
    assert!(uuids.contains(&"a-1"), "assistant message lands on main");
}
