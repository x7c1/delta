use delta_model::{Message, MessageUuid, ThreadId};
use delta_usecase::{AgentContentSource, AgentEvent, Effect};

use super::CodexConversationSource;

impl AgentContentSource for CodexConversationSource {
    /// Fold one neutral [`AgentEvent`] into the batch the persistence pipeline
    /// consumes.
    ///
    /// Delegates to the inherent [`CodexConversationSource::ingest`], which
    /// produces the messages the event completed. Codex emits no neutral
    /// [`Effect`]s through this content seam: the turn-end / permission
    /// correlation the effects encode is driven off the control stream
    /// (`events()` → the Turn FSM / permission reducers), not the content fold,
    /// so the batch is messages-only — the effect list is always empty.
    fn ingest(&mut self, event: &AgentEvent) -> (Vec<Message>, Vec<Effect>) {
        (CodexConversationSource::ingest(self, event), Vec::new())
    }

    /// Route the turn about to dispatch: land its messages on `thread_id` (the
    /// branch child thread for a branch send, `main` otherwise) and, for a branch
    /// send, stamp `semantic_parent` on the turn's root user message so the branch
    /// content matches the `send` row's own lane + parent. Set on the mailbox
    /// before the turn's frames are pumped in, so every message this turn folds
    /// uses it.
    fn begin_turn(&mut self, thread_id: ThreadId, semantic_parent: Option<MessageUuid>) {
        self.turn_thread = thread_id;
        self.pending_semantic_parent = semantic_parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::codex_content_source;
    use crate::content::testing::request;
    use delta_model::Role;

    #[test]
    fn the_content_source_trait_yields_the_messages_and_no_effects() {
        // Drive through the domain-side `AgentContentSource` seam (the shape the
        // pump holds), built by the Delta-side factory. It must return the same
        // messages the inherent fold produces, plus an empty effect list.
        let mut src = codex_content_source(request("sess-1", ThreadId(1), 7), None);
        let (messages, effects) = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "hi".to_owned(),
            at_ms: None,
        });
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].uuid, MessageUuid::from("codex-item-item_1"));
        assert_eq!(messages[0].role, Role::Assistant);
        assert_eq!(messages[0].seq, 7, "the factory's seed_seq is honoured");
        assert!(
            effects.is_empty(),
            "Codex emits no neutral effects through the content seam"
        );
    }

    #[test]
    fn the_content_source_trait_returns_an_empty_batch_for_control_events() {
        let mut src = codex_content_source(request("s", ThreadId(1), 0), None);
        let (messages, effects) = src.ingest(&AgentEvent::TurnStarted {
            provider_turn_id: Some("turn_1".to_owned()),
        });
        assert!(messages.is_empty());
        assert!(effects.is_empty());
    }

    #[test]
    fn a_branch_turn_routes_its_messages_to_the_branch_thread_and_stamps_the_semantic_parent() {
        // `begin_turn` sets the branch child thread + the branched-from message
        // before the turn's frames arrive (as the dispatch does on the mailbox).
        let mut src = CodexConversationSource::new(request("s", ThreadId(8), 0), None);
        src.begin_turn(
            ThreadId(9),
            Some(MessageUuid::from("codex-item-msg_parent")),
        );

        // The root user prompt lands on the branch thread AND carries the
        // semantic parent — reproducing (as the fix) the DB symptom, where these
        // rows wrongly landed on main (thread 8) with no semantic parent.
        let user = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "branch text".to_owned(),
            at_ms: None,
        });
        assert_eq!(user[0].role, Role::User);
        assert_eq!(
            user[0].thread_id,
            ThreadId(9),
            "the branch root user message lands on the branch thread, not main"
        );
        assert_eq!(
            user[0].semantic_parent_uuid,
            Some(MessageUuid::from("codex-item-msg_parent")),
            "the branch root user message is stamped with the branched-from message"
        );

        // The turn's subsequent assistant message also lands on the branch
        // thread, but does NOT re-carry the semantic parent (only the root does).
        let asst = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "reply".to_owned(),
            at_ms: None,
        });
        assert_eq!(
            asst[0].thread_id,
            ThreadId(9),
            "the branch turn's assistant reply also lands on the branch thread"
        );
        assert!(
            asst[0].semantic_parent_uuid.is_none(),
            "only the branch root carries the semantic parent, not later messages"
        );
    }

    #[test]
    fn a_plain_turn_stays_on_main_with_no_semantic_parent() {
        // With no `begin_turn`, or `begin_turn(main, None)`, every message stays
        // on the main thread with no semantic parent — the pre-fix behaviour a
        // non-branching session must keep byte-for-byte.
        let mut src = CodexConversationSource::new(request("s", ThreadId(8), 0), None);
        src.begin_turn(ThreadId(8), None);
        let user = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "hi".to_owned(),
            at_ms: None,
        });
        let asst = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "yo".to_owned(),
            at_ms: None,
        });
        assert_eq!(user[0].thread_id, ThreadId(8));
        assert!(user[0].semantic_parent_uuid.is_none());
        assert_eq!(asst[0].thread_id, ThreadId(8));
        assert!(asst[0].semantic_parent_uuid.is_none());
    }

    #[test]
    fn a_plain_turn_after_a_branch_turn_resets_back_to_main() {
        // A branch turn overrides the routing; the following plain turn's
        // `begin_turn(main, None)` must reset it, so late/next-turn content does
        // not leak onto the branch lane.
        let mut src = CodexConversationSource::new(request("s", ThreadId(8), 0), None);
        src.begin_turn(
            ThreadId(9),
            Some(MessageUuid::from("codex-item-msg_parent")),
        );
        let _ = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "branch".to_owned(),
            at_ms: None,
        });
        src.begin_turn(ThreadId(8), None);
        let user = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "plain again".to_owned(),
            at_ms: None,
        });
        assert_eq!(user[0].thread_id, ThreadId(8), "reset back to main");
        assert!(
            user[0].semantic_parent_uuid.is_none(),
            "the reset turn carries no semantic parent"
        );
    }
}
