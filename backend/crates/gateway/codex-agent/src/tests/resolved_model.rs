//! The model a launched or resumed session's messages report.

use super::*;

/// A content-source request for a session launched in `/work/app` on `branch` —
/// the neutral half of what the pump hands the adapter at bind time. It carries
/// the launch site Delta resolved and observed; the model the server reported is
/// the adapter's to add.
fn content_request(branch: Option<&str>) -> ContentSourceRequest {
    ContentSourceRequest {
        session_id: SessionId::from("01920000-0000-7000-8000-00000000000a"),
        main_thread: ThreadId(1),
        seed_seq: 0,
        cwd: "/work/app".to_owned(),
        git_branch: branch.map(str::to_owned),
    }
}

/// The one assistant message the source folds from a single completed item, for
/// asserting the provider metadata stamped on it.
fn folded_message(source: &mut Box<dyn AgentContentSource>) -> Message {
    let (messages, _) = source.ingest(&AgentEvent::AssistantMessage {
        provider_item_id: "item_1".to_owned(),
        text: "hi".to_owned(),
        at_ms: None,
    });
    messages.into_iter().next().expect("one folded message")
}

/// A launched session's messages report the model the **server** resolved, not
/// the one Delta asked for.
///
/// The launch selects one model and the server answers with a **different** one
/// — exactly what happens when the user's own Codex config or the server's
/// default wins over (or renames) the requested model. That divergence is the
/// point of the test, so the requested value is deliberately a synthetic string
/// that no catalog contains (`requested-by-delta`): were it a real slug, a
/// future edit could quietly align it with the resolved one and the test would
/// keep passing while proving nothing. The value that reaches the message is the
/// server's, so the transcript always shows what is really running.
///
/// `cwd` and `git_branch` ride the neutral request — Codex reports neither, so
/// they are Delta's own launch site — and `response_time_ms` stays `None`, since
/// Codex exposes no per-message latency.
#[tokio::test]
async fn a_launched_sessions_messages_carry_the_model_the_server_resolved() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        let start = server.next_frame().await;
        assert_eq!(start["method"], "thread/start");
        assert_eq!(
            start["params"]["model"], "requested-by-delta",
            "the selected launch option rode the request"
        );
        // The real `ThreadStartResponse` carries `model` at the top level.
        // Answer with a DIFFERENT model than was requested.
        server
            .send(json!({
                "id": start["id"],
                "result": { "thread": { "id": "thr_m" }, "model": "gpt-5.6-sol" }
            }))
            .await;
        // Hold the server side open until the test drops it.
        server
    });

    let handle = adapter
        .launch(LaunchRequest {
            session_id: "01920000-0000-7000-8000-00000000000a".to_owned(),
            workdir: "/work/app".to_owned(),
            launch_options: vec![option("model", Some("requested-by-delta"))],
            first_prompt: None,
            worktree_repo_root: None,
        })
        .await
        .expect("launch");
    let _server = tokio::time::timeout(TIMEOUT, server_task)
        .await
        .expect("server task timed out")
        .expect("server task panicked");

    let mut source = adapter.content_source(&handle, content_request(Some("feature/x")));
    let message = folded_message(&mut source);
    assert_eq!(
        message.model.as_deref(),
        Some("gpt-5.6-sol"),
        "the message reports the model the server resolved, never the requested one"
    );
    assert_eq!(
        message.git_branch.as_deref(),
        Some("feature/x"),
        "the launch site's branch rides the neutral request onto the message"
    );
    assert_eq!(message.cwd.as_deref(), Some("/work/app"));
    assert!(
        message.response_time_ms.is_none(),
        "Codex exposes no per-message latency, so it degrades to None"
    );
}

/// A **resumed** session reports its metadata too: `thread/resume` carries the
/// same required top-level `model` as `thread/start`, so a session picked back
/// up after a restart is not left blank.
#[tokio::test]
async fn a_resumed_sessions_messages_carry_the_model_the_resume_reported() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        let resume = server.next_frame().await;
        assert_eq!(resume["method"], "thread/resume");
        assert_eq!(resume["params"]["threadId"], "thr_m");
        server
            .send(json!({
                "id": resume["id"],
                "result": { "thread": { "id": "thr_m" }, "model": "gpt-5.6-sol" }
            }))
            .await;
        server
    });

    let handle = adapter
        .resume(ResumeRequest {
            session_id: "01920000-0000-7000-8000-00000000000a".to_owned(),
            provider_session_id: "thr_m".to_owned(),
            workdir: "/work/app".to_owned(),
            worktree_repo_root: None,
        })
        .await
        .expect("resume");
    let _server = tokio::time::timeout(TIMEOUT, server_task)
        .await
        .expect("server task timed out")
        .expect("server task panicked");

    let mut source = adapter.content_source(&handle, content_request(Some("feature/x")));
    let message = folded_message(&mut source);
    assert_eq!(
        message.model.as_deref(),
        Some("gpt-5.6-sol"),
        "a resumed session reports the model its resume response announced"
    );
    assert_eq!(
        message.git_branch.as_deref(),
        Some("feature/x"),
        "a resumed session's launch site is re-observed and reported too"
    );
    assert_eq!(message.cwd.as_deref(), Some("/work/app"));
}

/// A server answering without a `model`, for a session launched outside a git
/// working tree, leaves both facts absent rather than substituting something
/// plausible — the same degrade-never-fake rule the rest of the Codex fold
/// follows. The launch directory, which Delta always knows, is still reported.
#[tokio::test]
async fn a_response_without_a_model_degrades_to_none() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        let start = server.next_frame().await;
        server
            .send(json!({ "id": start["id"], "result": { "thread": { "id": "thr_n" } } }))
            .await;
        server
    });

    let handle = adapter
        .launch(LaunchRequest {
            session_id: "01920000-0000-7000-8000-00000000000a".to_owned(),
            workdir: "/work/app".to_owned(),
            launch_options: Vec::new(),
            first_prompt: None,
            worktree_repo_root: None,
        })
        .await
        .expect("launch");
    let _server = tokio::time::timeout(TIMEOUT, server_task)
        .await
        .expect("server task timed out")
        .expect("server task panicked");

    let mut source = adapter.content_source(&handle, content_request(None));
    let message = folded_message(&mut source);
    assert!(
        message.model.is_none(),
        "no model in the response means no model on the message"
    );
    assert!(
        message.git_branch.is_none(),
        "no branch observed at the launch site means none on the message"
    );
    assert_eq!(
        message.cwd.as_deref(),
        Some("/work/app"),
        "the launch directory is Delta's own record, so it is reported regardless"
    );
}
