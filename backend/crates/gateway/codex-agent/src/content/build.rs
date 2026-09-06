use delta_model::{ContentBlock, Message, MessageUuid, Role};

use super::CodexConversationSource;

impl CodexConversationSource {
    /// Assemble a canonical message, minting the next `seq` and degrading every
    /// provider fact Codex does not expose to `None`.
    ///
    /// `at_ms` is the item's lifecycle timestamp in epoch milliseconds (the
    /// `startedAtMs` / `completedAtMs` the translation carried onto the event),
    /// converted here to the canonical ISO-8601 UTC `created_at` string. It stays
    /// `None` when the provider exposed no time, so `created_at` degrades rather
    /// than being invented. `model` / `git_branch` / `cwd` are copied from the
    /// session metadata captured at construction (see
    /// [`CodexConversationSource::new`]); `response_time_ms` stays `None` — Codex
    /// exposes no per-message latency and inferring one from item timestamps
    /// would be a different measurement than Claude's, so it degrades.
    pub(super) fn build(
        &mut self,
        uuid: MessageUuid,
        role: Role,
        content: Vec<ContentBlock>,
        provider_item_id: Option<String>,
        semantic_parent_uuid: Option<MessageUuid>,
        at_ms: Option<i64>,
    ) -> Message {
        let seq = self.next_seq;
        self.next_seq += 1;
        Message {
            uuid,
            provider_item_id,
            session_id: self.session_id.clone(),
            // The current turn's thread: `main` for a plain turn, or the branch
            // child thread `begin_turn` set for a branch turn — so branch content
            // lands on the branch lane, not `main`.
            thread_id: self.turn_thread,
            role,
            linear_parent_uuid: None,
            semantic_parent_uuid,
            prompt_id: self.current_turn.clone(),
            seq,
            content_text: Message::flatten_text(&content),
            content,
            created_at: at_ms.and_then(iso8601_from_epoch_ms),
            model: self.model.clone(),
            git_branch: self.git_branch.clone(),
            cwd: Some(self.cwd.clone()),
            response_time_ms: None,
        }
    }
}

/// Convert an epoch-millisecond timestamp to the canonical ISO-8601 UTC string
/// Delta stores in [`Message::created_at`] (the same RFC 3339 `…Z` shape Claude's
/// transcript timestamps already use). An out-of-range value yields `None` rather
/// than a bogus string.
fn iso8601_from_epoch_ms(at_ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(at_ms)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::testing::{request, source, TEST_CWD};
    use delta_model::{SessionId, ThreadId};
    use delta_usecase::{AgentEvent, ContentSourceRequest};
    use serde_json::json;

    /// The session's provider metadata is stamped on **every** message the fold
    /// produces — the user prompt, the assistant reply, a paired tool call, and a
    /// tool flushed at turn end — and stays put across turns, because it is a
    /// per-session fact captured once rather than something re-read per event.
    #[test]
    fn every_folded_message_carries_the_sessions_model_cwd_and_branch() {
        let mut src = CodexConversationSource::new(
            ContentSourceRequest {
                session_id: SessionId::from("sess-1"),
                main_thread: ThreadId(1),
                seed_seq: 0,
                cwd: "/work/app".to_owned(),
                git_branch: Some("feature/x".to_owned()),
            },
            Some("gpt-5.6-sol".to_owned()),
        );

        let mut folded = Vec::new();
        folded.extend(src.ingest(&AgentEvent::TurnStarted {
            provider_turn_id: Some("turn_1".to_owned()),
        }));
        folded.extend(src.ingest(&AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "hello".to_owned(),
            at_ms: None,
        }));
        folded.extend(src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "hi".to_owned(),
            at_ms: None,
        }));
        src.ingest(&AgentEvent::ToolStarted {
            provider_item_id: "t1".to_owned(),
            name: "Bash".to_owned(),
            input_json: json!({ "command": "ls" }),
            at_ms: None,
        });
        folded.extend(src.ingest(&AgentEvent::ToolCompleted {
            provider_item_id: "t1".to_owned(),
            output_json: json!({ "exitCode": 0 }),
            at_ms: None,
        }));
        // A tool left open at turn end is flushed — that path stamps too.
        src.ingest(&AgentEvent::ToolStarted {
            provider_item_id: "t2".to_owned(),
            name: "Bash".to_owned(),
            input_json: json!({}),
            at_ms: None,
        });
        folded.extend(src.ingest(&AgentEvent::TurnCompleted {
            status: delta_usecase::TurnStatus::Completed,
        }));
        // A second turn still reports the same session facts.
        folded.extend(src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_2".to_owned(),
            text: "again".to_owned(),
            at_ms: None,
        }));

        assert_eq!(folded.len(), 5, "prompt + reply + tool + flush + reply");
        for m in &folded {
            assert_eq!(
                m.model.as_deref(),
                Some("gpt-5.6-sol"),
                "message {} reports the session's model",
                m.uuid.as_str()
            );
            assert_eq!(m.cwd.as_deref(), Some("/work/app"));
            assert_eq!(m.git_branch.as_deref(), Some("feature/x"));
        }
    }

    /// A session with no model reported and no branch observed still reports the
    /// one fact Delta always knows — where the agent is running — and degrades
    /// the other two rather than inventing them.
    #[test]
    fn absent_provider_metadata_degrades_but_the_launch_directory_is_always_reported() {
        let mut src = source();
        let msgs = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "hi".to_owned(),
            at_ms: None,
        });
        let m = &msgs[0];
        assert!(m.model.is_none(), "no model reported means none stamped");
        assert!(
            m.git_branch.is_none(),
            "no branch observed means no branch stamped"
        );
        assert_eq!(m.cwd.as_deref(), Some(TEST_CWD));
    }

    #[test]
    fn an_events_at_ms_becomes_an_iso8601_created_at_and_absence_degrades_to_none() {
        let mut src = source();
        // A message built from an event carrying `at_ms` gets a canonical
        // ISO-8601 UTC `created_at` (RFC 3339, `…Z`), converted from epoch ms.
        let with_ts = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "a1".to_owned(),
            text: "hi".to_owned(),
            at_ms: Some(1_700_000_000_123),
        });
        assert_eq!(
            with_ts[0].created_at.as_deref(),
            Some("2023-11-14T22:13:20.123Z")
        );
        // A missing `at_ms` still degrades `created_at` to None (never invented).
        let without_ts = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "a2".to_owned(),
            text: "yo".to_owned(),
            at_ms: None,
        });
        assert!(without_ts[0].created_at.is_none());
    }

    #[test]
    fn a_completed_tools_created_at_comes_from_the_completion_and_a_flush_from_the_start() {
        // A paired tool message is minted at completion, so its `created_at` is
        // the completion's `at_ms`.
        let mut src = source();
        src.ingest(&AgentEvent::ToolStarted {
            provider_item_id: "t1".to_owned(),
            name: "Bash".to_owned(),
            input_json: json!({ "command": "ls" }),
            at_ms: Some(1_700_000_000_000),
        });
        let completed = src.ingest(&AgentEvent::ToolCompleted {
            provider_item_id: "t1".to_owned(),
            output_json: json!({ "exitCode": 0 }),
            at_ms: Some(1_700_000_005_000),
        });
        assert_eq!(
            completed[0].created_at.as_deref(),
            Some("2023-11-14T22:13:25.000Z")
        );

        // A tool left open at turn end is flushed with its `ToolStarted` time.
        src.ingest(&AgentEvent::ToolStarted {
            provider_item_id: "t2".to_owned(),
            name: "Bash".to_owned(),
            input_json: json!({}),
            at_ms: Some(1_700_000_000_000),
        });
        let flushed = src.ingest(&AgentEvent::TurnCompleted {
            status: delta_usecase::TurnStatus::Completed,
        });
        assert_eq!(
            flushed[0].created_at.as_deref(),
            Some("2023-11-14T22:13:20.000Z")
        );
    }

    #[test]
    fn seed_seq_continues_past_persisted_messages() {
        let mut src = CodexConversationSource::new(request("s", ThreadId(1), 42), None);
        let msgs = src.ingest(&AgentEvent::AssistantMessage {
            provider_item_id: "i".to_owned(),
            text: "x".to_owned(),
            at_ms: None,
        });
        assert_eq!(msgs[0].seq, 42);
    }
}
