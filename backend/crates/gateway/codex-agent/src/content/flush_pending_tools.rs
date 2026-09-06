use delta_model::{ContentBlock, Message, Role};

use super::{item_uuid, CodexConversationSource};

impl CodexConversationSource {
    /// Flush any tool calls left open at turn end as `ToolUse`-only messages, so
    /// a call whose completion never arrived is still recorded rather than
    /// dropped. Drained in ascending `provider_item_id` order for determinism.
    pub(super) fn flush_pending_tools(&mut self) -> Vec<Message> {
        if self.pending_tools.is_empty() {
            return Vec::new();
        }
        let mut ids: Vec<String> = self.pending_tools.keys().cloned().collect();
        ids.sort();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let started = self
                .pending_tools
                .remove(&id)
                .expect("id came from the map's own keys");
            let block = ContentBlock::ToolUse {
                id: id.clone(),
                name: started.name,
                input: started.input,
            };
            let uuid = item_uuid(&id);
            out.push(self.build(
                uuid,
                Role::Assistant,
                vec![block],
                Some(id),
                None,
                started.at_ms,
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::testing::source;
    use delta_usecase::AgentEvent;
    use serde_json::json;

    #[test]
    fn a_tool_left_open_at_turn_end_flushes_as_tool_use_only() {
        let mut src = source();
        src.ingest(&AgentEvent::ToolStarted {
            provider_item_id: "t1".to_owned(),
            name: "Bash".to_owned(),
            input_json: json!({ "command": "sleep 100" }),
            at_ms: None,
        });
        let flushed = src.ingest(&AgentEvent::TurnCompleted {
            status: delta_usecase::TurnStatus::Completed,
        });
        assert_eq!(flushed.len(), 1);
        assert_eq!(
            flushed[0].content,
            vec![ContentBlock::ToolUse {
                id: "t1".to_owned(),
                name: "Bash".to_owned(),
                input: json!({ "command": "sleep 100" }),
            }]
        );
    }
}
