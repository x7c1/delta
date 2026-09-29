//! A prompt pasted straight into the pane is announced without Claude Code's
//! `<pasted_content>` wrapper.
//!
//! Recent Claude Code builds wrap a paste of 20 or more characters in a
//! `<pasted_content id="xxxx">` block before submitting it, and the
//! `UserPromptSubmit` hook's `prompt` carries that wrapped form. When the
//! prompt consumed no send of Delta's, the `ExternalInput` notice shows the
//! prompt to the user, so it must show what they pasted — not the transport
//! tags around it. Text the user typed next to the block is theirs and is
//! kept; only the tags and the newlines the wrapper added go.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// A paste of 20+ characters, as the user pasted it.
const BODY: &str = "a pasted passage well over twenty characters\nwith a second line";

/// [`BODY`] wrapped exactly as Claude Code submits a prompt that is nothing
/// but the paste.
const WRAPPED: &str = "\n\n<pasted_content id=\"3b9c\">\na pasted passage well over twenty characters\nwith a second line\n</pasted_content id=\"3b9c\">\n";

/// The `prompt` of the one `ExternalInput` event among `events`.
fn external_input_prompt(events: &[SessionEvent]) -> &str {
    let prompts: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            SessionEvent::ExternalInput { prompt, .. } => Some(prompt.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(prompts.len(), 1, "expected one ExternalInput in {events:?}");
    prompts[0]
}

#[tokio::test]
async fn external_input_unwraps_pasted_content() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();

    // Nothing of Delta's is outstanding for this session, so whatever is
    // submitted next was put into the pane from outside Delta.
    assert!(ix
        .store()
        .head_dispatched_send(&session)
        .await
        .unwrap()
        .is_none());

    // A prompt that is nothing but the wrapped paste: the notice carries the
    // bare body.
    ix.transcript_fake().push(user_line("u-paste", WRAPPED));
    let (events, additional) = ix.on_user_prompt_submit(submit(WRAPPED)).await.unwrap();
    assert!(additional.is_none());
    assert_eq!(external_input_prompt(&events), BODY);

    // Text typed next to the block is kept; only the wrapper goes.
    let mixed = format!("please look at this{WRAPPED}\nwhat do you think?");
    ix.transcript_fake().push(user_line("u-mixed", &mixed));
    let (events, additional) = ix.on_user_prompt_submit(submit(&mixed)).await.unwrap();
    assert!(additional.is_none());
    assert_eq!(
        external_input_prompt(&events),
        format!("please look at this{BODY}\nwhat do you think?"),
    );
}
