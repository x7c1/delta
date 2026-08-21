//! Fixtures the behaviour modules share: the scenario file on disk, the two
//! adapter builders over it, the event-collection helper, and the turn scenario
//! most cases script.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use codex_agent::{AppServerConnection, CodexAppServerAdapter, CodexLaunchConfig};
use delta_usecase::{AgentEvent, AgentEventStream};
use serde_json::json;
use tokio::time::timeout;

/// A short bound so a wiring bug fails fast instead of hanging the suite.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);

/// A scenario file written to a unique temp dir, removed on drop.
pub(crate) struct ScenarioGuard {
    dir: PathBuf,
    path: PathBuf,
}

impl ScenarioGuard {
    pub(crate) fn write(scenario: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("fake-codex-adapter-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scenario.json");
        std::fs::write(&path, scenario).unwrap();
        Self { dir, path }
    }
}

impl Drop for ScenarioGuard {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// Spawn the fake with an explicit scenario and build an initialised adapter
/// over the connection. The guard must be kept alive for the fake's lifetime
/// (it owns the scenario file on disk).
pub(crate) async fn adapter_with(scenario: &str) -> (CodexAppServerAdapter, ScenarioGuard) {
    let guard = ScenarioGuard::write(scenario);
    let config = CodexLaunchConfig {
        codex_bin: env!("CARGO_BIN_EXE_fake-codex").to_owned(),
        args: vec![],
        env: vec![(
            "FAKE_CODEX_SCENARIO".to_owned(),
            guard.path.to_string_lossy().into_owned(),
        )],
    };
    let conn = Arc::new(AppServerConnection::spawn(&config).expect("spawn fake-codex"));
    conn.initialize(json!({ "clientInfo": { "name": "delta", "version": "0" } }))
        .await
        .expect("initialize");
    (CodexAppServerAdapter::new(conn), guard)
}

/// Spawn the fake with its built-in default scenario (one short assistant-message
/// turn), used by the mechanical shared cases.
pub(crate) async fn default_adapter() -> CodexAppServerAdapter {
    let config = CodexLaunchConfig {
        codex_bin: env!("CARGO_BIN_EXE_fake-codex").to_owned(),
        args: vec![],
        env: vec![],
    };
    let conn = Arc::new(AppServerConnection::spawn(&config).expect("spawn fake-codex"));
    conn.initialize(json!({ "clientInfo": { "name": "delta", "version": "0" } }))
        .await
        .expect("initialize");
    CodexAppServerAdapter::new(conn)
}

/// Receive events until `stop` returns true for one (inclusive), or the per-event
/// timeout fires (a hang — which is itself a contract failure). The stream
/// closing early also stops the collection.
pub(crate) async fn collect_until<F>(stream: &mut AgentEventStream, stop: F) -> Vec<AgentEvent>
where
    F: Fn(&AgentEvent) -> bool,
{
    let mut events = Vec::new();
    loop {
        match timeout(TIMEOUT, stream.recv()).await {
            Ok(Some(event)) => {
                let done = stop(&event);
                events.push(event);
                if done {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => panic!("timed out waiting for an event; collected so far: {events:?}"),
        }
    }
    events
}

pub(crate) fn is_turn_completed(event: &AgentEvent) -> bool {
    matches!(event, AgentEvent::TurnCompleted { .. })
}

/// A turn scenario that emits a full assistant-message turn, with an optional
/// extra emission spliced in before completion.
pub(crate) fn turn_scenario(extra_emit: &str) -> String {
    format!(
        r#"{{
            "thread_id": "thr_contract",
            "turn": {{
                "turn_id": "turn_contract",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started", "item": {{ "id": "m1", "type": "agentMessage" }} }},
                    {{ "type": "agent_message_delta", "item_id": "m1", "delta": "scripted reply" }},
                    {extra_emit}
                    {{ "type": "item_completed", "item": {{ "id": "m1", "type": "agentMessage", "text": "scripted reply" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#
    )
}
