use delta_model::MessageUuid;

use super::CodexConversationSource;

impl CodexConversationSource {
    /// Synthesize a stable uuid for a user prompt. Codex gives the accepted
    /// prompt no item id, so it is keyed off the `seq` this message will be
    /// minted with.
    ///
    /// Keying it off `next_seq` — rather than a per-source counter — makes it
    /// unique across the session's whole sequence space, including after a
    /// **resume** that re-seeds `next_seq` at the persisted `MAX(seq) + 1`. A
    /// counter would reset to 0 on the fresh post-resume source and collide its
    /// first prompt's uuid with the pre-restart first prompt (`codex-user-0`),
    /// silently overwriting that earlier message.
    ///
    /// It is deliberately **not** keyed off the turn id, even though one is
    /// usually known here: at `UserPromptAccepted` time that id is never this
    /// prompt's own turn, and one turn can accept several prompts — so a
    /// turn-keyed uuid collides across prompts and the upsert loses one of them
    /// (see *A prompt arrives before its turn is named* in the
    /// [`crate::content`] module docs).
    pub(super) fn user_prompt_uuid(&self) -> MessageUuid {
        MessageUuid::from(format!("codex-user-{}", self.next_seq))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::testing::{request, source};
    use delta_model::{PromptId, ThreadId};
    use delta_usecase::{AgentContentSource, AgentEvent};

    /// Codex accepts a `turn/start` while a turn is in flight and steers the
    /// input into the running turn, so two prompts can be folded under one turn
    /// id — and the second may be a branch send routed by `begin_turn`. Each
    /// must keep its own identity and its own lane: a turn-keyed uuid made them
    /// collide, and the upsert then dropped the first prompt's text while
    /// pinning the survivor to the first prompt's thread.
    #[test]
    fn two_prompts_accepted_under_one_turn_id_get_distinct_uuids_and_keep_their_own_lane() {
        let mut src = CodexConversationSource::new(request("s", ThreadId(8), 0), None);
        src.ingest(&AgentEvent::TurnStarted {
            provider_turn_id: Some("turn_1".to_owned()),
        });

        // A plain send on main.
        src.begin_turn(ThreadId(8), None);
        let plain = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "first".to_owned(),
            at_ms: None,
        });
        // A branch send dispatched while that same turn is still running: no
        // `TurnStarted` arrives for it, because Codex steers it into the turn.
        src.begin_turn(
            ThreadId(9),
            Some(MessageUuid::from("codex-item-msg_parent")),
        );
        let branch = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "second".to_owned(),
            at_ms: None,
        });

        assert_ne!(
            plain[0].uuid, branch[0].uuid,
            "two prompts under one turn must not share a uuid, or one overwrites the other"
        );
        assert_eq!(plain[0].thread_id, ThreadId(8));
        assert!(plain[0].semantic_parent_uuid.is_none());
        assert_eq!(
            branch[0].thread_id,
            ThreadId(9),
            "the branch prompt keeps the lane `begin_turn` routed it onto"
        );
        assert_eq!(
            branch[0].semantic_parent_uuid,
            Some(MessageUuid::from("codex-item-msg_parent")),
        );
        // Both really are that turn's input, so both carry its group.
        assert_eq!(plain[0].prompt_id, Some(PromptId::from("turn_1")));
        assert_eq!(branch[0].prompt_id, Some(PromptId::from("turn_1")));
    }

    #[test]
    fn two_prompt_less_turns_get_distinct_user_uuids() {
        let mut src = source();
        let a = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "one".to_owned(),
            at_ms: None,
        });
        let b = src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "two".to_owned(),
            at_ms: None,
        });
        assert_ne!(a[0].uuid, b[0].uuid);
        assert_eq!(a[0].uuid, MessageUuid::from("codex-user-0"));
        assert_eq!(b[0].uuid, MessageUuid::from("codex-user-1"));
    }

    #[test]
    fn a_resumed_sources_first_prompt_does_not_collide_with_the_pre_restart_one() {
        // A fresh source seeds at 0: its first prompt-less user prompt is
        // `codex-user-0` at seq 0.
        let fresh_uuid = source()
            .ingest(&AgentEvent::UserPromptAccepted {
                provider_message_id: None,
                text: "first message".to_owned(),
                at_ms: None,
            })
            .remove(0)
            .uuid;
        assert_eq!(fresh_uuid, MessageUuid::from("codex-user-0"));

        // After a restart the source is re-seeded at the persisted count (2 here).
        // Its first prompt-less user prompt must NOT reuse `codex-user-0` — that
        // would overwrite the pre-restart message — so it is keyed off the seeded
        // seq (2) instead, and lands at seq 2.
        let mut resumed = CodexConversationSource::new(request("sess-1", ThreadId(1), 2), None);
        let resumed_msg = resumed
            .ingest(&AgentEvent::UserPromptAccepted {
                provider_message_id: None,
                text: "second message".to_owned(),
                at_ms: None,
            })
            .remove(0);
        assert_ne!(
            resumed_msg.uuid, fresh_uuid,
            "the resumed prompt must not collide with the pre-restart one"
        );
        assert_eq!(resumed_msg.uuid, MessageUuid::from("codex-user-2"));
        assert_eq!(
            resumed_msg.seq, 2,
            "and it continues the persisted sequence"
        );
    }
}
