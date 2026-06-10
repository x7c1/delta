use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

#[tokio::test]
async fn first_submit_registers_session() {
    let ix = interactor();
    let (events, _) = ix.on_user_prompt_submit(submit("hi")).await.unwrap();
    assert!(events.contains(&SessionEvent::SessionRegistered {
        session_id: SessionId::from("sess-1"),
    }));
    assert!(ix
        .store()
        .session(&SessionId::from("sess-1"))
        .await
        .unwrap()
        .is_some());
}
