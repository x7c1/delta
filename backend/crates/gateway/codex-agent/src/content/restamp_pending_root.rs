use delta_model::Message;

use super::CodexConversationSource;

impl CodexConversationSource {
    /// Re-emit the prompt that was accepted before this turn announced itself,
    /// now carrying the turn as its `prompt_id`.
    ///
    /// The prompt is built (and returned) at `UserPromptAccepted` time, before
    /// `turn/start` is even issued, so it cannot know its own turn id then. This
    /// yields the identical message — same uuid, `seq`, thread and semantic
    /// parent — with the group filled in, which the persistence upsert folds
    /// onto the existing row (`prompt_id` is one of the columns it refreshes)
    /// and the browser picks up on its `transcript_updated` refetch.
    ///
    /// The pending root is consumed either way, so only the turn that directly
    /// follows the prompt can claim it: a turn announced without an id leaves
    /// the prompt's group degraded to `None` rather than inventing one.
    pub(super) fn restamp_pending_root(&mut self) -> Vec<Message> {
        let Some(mut root) = self.pending_turn_root.take() else {
            return Vec::new();
        };
        match &self.current_turn {
            Some(turn) => {
                root.prompt_id = Some(turn.clone());
                vec![root]
            }
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::testing::{request, source};
    use delta_model::{MessageUuid, PromptId, Role, ThreadId};
    use delta_usecase::AgentEvent;

    #[test]
    fn a_turn_id_becomes_the_prompt_group_and_seq_is_monotonic() {
        // The live ordering, which the adapter guarantees: the prompt's
        // acceptance is emitted before `turn/start` is issued, so the prompt is
        // folded first and its turn names itself only afterwards.
        let mut src = source();
        let user = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "do it".to_owned(),
            at_ms: None,
        });
        assert_eq!(user[0].role, Role::User);
        assert_eq!(user[0].uuid, MessageUuid::from("codex-user-0"));
        assert_eq!(user[0].seq, 0);
        assert_eq!(
            user[0].prompt_id, None,
            "the turn it was sent to has not named itself yet"
        );

        // Its `TurnStarted` re-emits the same prompt with the turn as its group.
        let restamped = src.ingest(&AgentEvent::TurnStarted {
            provider_turn_id: Some("turn_9".to_owned()),
        });
        assert_eq!(restamped.len(), 1);
        assert_eq!(restamped[0].uuid, user[0].uuid);
        assert_eq!(restamped[0].seq, 0, "the re-emit keeps the prompt's seq");
        assert_eq!(restamped[0].prompt_id, Some(PromptId::from("turn_9")));

        // The turn's own content is stamped with it directly, and `seq` keeps
        // advancing past the prompt.
        let asst = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "done".to_owned(),
            at_ms: None,
        });
        assert_eq!(asst[0].prompt_id, Some(PromptId::from("turn_9")));
        assert_eq!(asst[0].seq, 1);
    }

    #[test]
    fn a_prompt_accepted_before_its_turn_started_is_restamped_with_that_turn() {
        let mut src = CodexConversationSource::new(request("s", ThreadId(8), 0), None);
        // An idle source: the previous turn has completed, so no turn is open.
        src.ingest(&AgentEvent::TurnCompleted {
            status: delta_usecase::TurnStatus::Completed,
        });
        let user = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "do it".to_owned(),
            at_ms: None,
        });
        assert_eq!(
            user[0].prompt_id, None,
            "with no turn open the group degrades rather than borrowing the last turn's id"
        );

        let restamped = src.ingest(&AgentEvent::TurnStarted {
            provider_turn_id: Some("turn_2".to_owned()),
        });
        assert_eq!(restamped.len(), 1, "exactly one re-emit of the same prompt");
        assert_eq!(restamped[0].uuid, user[0].uuid);
        assert_eq!(restamped[0].seq, user[0].seq);
        assert_eq!(restamped[0].thread_id, user[0].thread_id);
        assert_eq!(restamped[0].content, user[0].content);
        assert_eq!(restamped[0].prompt_id, Some(PromptId::from("turn_2")));

        // The pending root is consumed, so a later turn re-emits nothing.
        assert!(src
            .ingest(&AgentEvent::TurnStarted {
                provider_turn_id: Some("turn_3".to_owned()),
            })
            .is_empty());
    }

    #[test]
    fn a_prompt_steered_into_a_running_turn_keeps_that_turn_and_is_not_restamped_by_the_next() {
        let mut src = source();
        src.ingest(&AgentEvent::TurnStarted {
            provider_turn_id: Some("turn_1".to_owned()),
        });
        let user = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "also do this".to_owned(),
            at_ms: None,
        });
        assert_eq!(
            user[0].prompt_id,
            Some(PromptId::from("turn_1")),
            "input steered into a running turn belongs to that turn"
        );

        assert!(src
            .ingest(&AgentEvent::TurnCompleted {
                status: delta_usecase::TurnStatus::Completed,
            })
            .is_empty());
        assert!(
            src.ingest(&AgentEvent::TurnStarted {
                provider_turn_id: Some("turn_2".to_owned()),
            })
            .is_empty(),
            "the next turn must not claim a prompt the finished turn already consumed"
        );
    }

    /// A `turn/started` that carries no `turn.id` translates to
    /// `TurnStarted { provider_turn_id: None }`, so the fold has no id to stamp.
    /// It still consumes the pending root: the prompt's group stays degraded to
    /// `None` rather than being invented, and no later, unrelated turn adopts it.
    #[test]
    fn a_turn_announced_without_an_id_leaves_the_prompt_ungrouped_and_consumes_it() {
        let mut src = source();
        let user = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "do it".to_owned(),
            at_ms: None,
        });
        assert_eq!(user[0].prompt_id, None);

        assert!(
            src.ingest(&AgentEvent::TurnStarted {
                provider_turn_id: None,
            })
            .is_empty(),
            "an id-less turn re-emits nothing — there is no group to fill in"
        );
        assert!(
            src.ingest(&AgentEvent::TurnStarted {
                provider_turn_id: Some("turn_2".to_owned()),
            })
            .is_empty(),
            "and the pending root is gone, so the following turn cannot claim it"
        );
    }
}
