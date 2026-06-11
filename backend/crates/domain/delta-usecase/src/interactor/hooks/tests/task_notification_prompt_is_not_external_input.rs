use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// A background-task completion notification is injected by the harness as a
/// `UserPromptSubmit` whose prompt begins with `<task-notification>`. It matches
/// no queued send, but it is a harness injection rather than pane typing, so it
/// must not surface as external input.
#[tokio::test]
async fn task_notification_prompt_is_not_external_input() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();

    let prompt = "<task-notification>\n\
                  <task-id>abc</task-id>\n\
                  <status>completed</status>\n\
                  </task-notification>";

    let (events, _additional) = ix.on_user_prompt_submit(submit(prompt)).await.unwrap();

    assert!(!events
        .iter()
        .any(|e| matches!(e, SessionEvent::ExternalInput { .. })));
}
