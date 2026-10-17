//! The forward migration of the launch row's running-indicator columns (v12):
//! what a re-adopted session reads to light the indicator again for a
//! background subagent that outlived a restart.

use delta_model::{ContentBlock, Message, MessageUuid, Role, SessionId, ThreadId};
use delta_usecase::RunningSubagentLaunch;

use super::super::super::SqliteStore;
use super::super::new_session;
use super::{downgrade_sql, read_user_version};

/// An assistant line carrying one `tool_use` block, as the message cache keeps
/// a launch line.
fn tool_use_message(
    session_id: &SessionId,
    thread_id: ThreadId,
    uuid: &str,
    seq: i64,
    block: ContentBlock,
) -> Message {
    Message {
        uuid: MessageUuid::from(uuid),
        session_id: session_id.clone(),
        thread_id,
        role: Role::Assistant,
        linear_parent_uuid: None,
        semantic_parent_uuid: None,
        prompt_id: None,
        seq,
        content_text: None,
        content: vec![block],
        created_at: Some("2026-01-01T00:00:00Z".into()),
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
        provider_item_id: None,
    }
}

/// A database written by the generation before the indicator columns —
/// stamped 11 — is migrated forward on open, and the launches it left
/// outstanding are backfilled from what the database already knows: an
/// `Agent` launch (found by its cached `tool_use` block) and a forked skill
/// (found by its synthetic id) read back as lighting the indicator, the
/// former with its display fields; a background `Bash` launch does not.
#[tokio::test]
async fn a_v11_database_backfills_which_outstanding_launches_lit_the_indicator() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v11.sqlite");
    let path_str = path.to_str().unwrap();

    let main = {
        let store = SqliteStore::open(path_str).unwrap();
        let (session, main) = store.register_session(new_session()).await.unwrap();
        store
            .upsert_messages(&[
                tool_use_message(
                    &session.id,
                    main,
                    "a-agent",
                    0,
                    ContentBlock::ToolUse {
                        id: "toolu_agent".into(),
                        name: "Agent".into(),
                        input: serde_json::json!({
                            "subagent_type": "general-purpose",
                            "description": "Long crawl",
                        }),
                    },
                ),
                tool_use_message(
                    &session.id,
                    main,
                    "a-task",
                    1,
                    ContentBlock::ToolUse {
                        id: "toolu_task".into(),
                        name: "Task".into(),
                        // A non-string field is not a display value.
                        input: serde_json::json!({ "description": 7 }),
                    },
                ),
                tool_use_message(
                    &session.id,
                    main,
                    "a-bash",
                    2,
                    ContentBlock::ToolUse {
                        id: "toolu_bash".into(),
                        name: "Bash".into(),
                        input: serde_json::json!({
                            "command": "sleep 600",
                            "run_in_background": true,
                        }),
                    },
                ),
            ])
            .await
            .unwrap();
        for tool_use_id in [
            "toolu_agent",
            "toolu_task",
            "toolu_bash",
            "forked-skill:agent_1",
            // A launch whose line never reached the message cache.
            "toolu_uncached",
        ] {
            store
                .record_subagent_launch(&session.id, tool_use_id, main)
                .await
                .unwrap();
        }
        store
            .upgrade_subagent_task_id(&session.id, "toolu_agent", "agent_abc")
            .await
            .unwrap();
        let conn = store.conn.lock().await;
        conn.execute_batch(&downgrade_sql(11)).unwrap();
        main
    };
    assert_eq!(read_user_version(path_str), 11);

    let store = SqliteStore::open(path_str).unwrap();
    assert_eq!(read_user_version(path_str), crate::SCHEMA_VERSION);

    let id = SessionId::from("sess-1");
    let mut lit = store.running_subagent_launches(&id).await.unwrap();
    lit.sort_by(|a, b| a.tool_use_id.cmp(&b.tool_use_id));
    assert_eq!(
        lit,
        vec![
            RunningSubagentLaunch {
                tool_use_id: "forked-skill:agent_1".into(),
                thread_id: main,
                task_id: None,
                subagent_type: None,
                description: None,
            },
            RunningSubagentLaunch {
                tool_use_id: "toolu_agent".into(),
                thread_id: main,
                task_id: Some("agent_abc".into()),
                subagent_type: Some("general-purpose".into()),
                description: Some("Long crawl".into()),
            },
            RunningSubagentLaunch {
                tool_use_id: "toolu_task".into(),
                thread_id: main,
                task_id: None,
                subagent_type: None,
                description: None,
            },
        ],
        "the subagent launches light the indicator, with defaults for what the \
         database does not know; the Bash and the uncached launch do not"
    );
    assert_eq!(
        store
            .outstanding_subagent_launches(&id)
            .await
            .unwrap()
            .len(),
        5,
        "every launch is still outstanding for attribution"
    );
}
