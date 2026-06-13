//! Real-claude contract canaries: drive the REAL `claude` CLI through tmux
//! with Delta's exact spawn shape and assert the implicit upstream contract
//! Delta depends on — the contract `fake-claude` re-enacts and the transcript
//! parser / hook handlers assume.
//!
//! These tests are **contract monitoring, not feature testing**: they exist
//! to detect upstream format/behavior drift that would silently break Delta's
//! transcript parsing and hook handling. When one breaks, the upstream
//! contract changed — update `fake-claude`'s scenario engine (and Delta's
//! parsing) to the new reality so the fake lane never drifts green. See
//! docs/guides/development.md ("End-to-end canaries (real claude)").
//!
//! Every test is `#[ignore]`: a run consumes the local user's Claude
//! subscription quota, so the suite only runs on demand via `make e2e-real`
//! (`cargo test … -- --ignored --test-threads=1`), never under `cargo test`
//! or in CI. Every canary uses the smallest workable prompt and assertions
//! are structural only (a line with this role/shape appeared; a hook with
//! these fields arrived) — never about response wording, which is
//! non-deterministic. Each canary allows exactly one retry for flakiness.
//!
//! What each canary pins:
//!
//! - [`prompt_turn_fires_hooks_and_streams_the_transcript`]:
//!   `SessionStart`(source=startup) / `UserPromptSubmit` / `Stop` arrive as
//!   HTTP POSTs deserializable by Delta's exact wire types; the user line and
//!   the assistant reply stream into the JSONL at `transcript_path` and parse
//!   with Delta's parser; the `UserPromptSubmit` `additionalContext` envelope
//!   is consumed; `/exit` writes an `isMeta` caveat line (`Role::Meta`) and
//!   fires `SessionEnd`. Also: no `PermissionRequest` fires for a turn with
//!   no permission dialog.
//! - [`interrupting_a_turn_writes_the_marker_and_queued_prompts_dequeue`]:
//!   Escape writes the `[Request interrupted by user…` marker as a
//!   `role: user` line (accepted by `claude_format::is_interrupt_marker`)
//!   and fires NO `Stop`; a prompt typed while the turn was busy is recorded
//!   and replayed after the interrupt, firing its own `UserPromptSubmit`.
//! - [`permission_dialog_fires_the_hook_and_the_allow_decision_is_honored`]:
//!   `PreToolUse` carries `tool_use_id`; `PermissionRequest` fires for a
//!   dialog-worthy tool call, carries `tool_name`/`tool_input` and — load
//!   bearing for Delta's row-ownership design — has **no** `tool_use_id`;
//!   answering with `hookSpecificOutput.decision.behavior = "allow"` lets the
//!   tool run (its `tool_result` lands with the `PreToolUse` id) and the turn
//!   completes.
//!
//! Environment notes:
//!
//! - The spawned `claude` strips the nested-session markers (`CLAUDECODE`,
//!   `CLAUDE_CODE_*`) from its environment: a `claude` that believes it is a
//!   child of another Claude Code session does **not** persist its JSONL
//!   transcript, which would break every transcript assertion (verified
//!   empirically on 2.1.x). Delta in production is launched from a normal
//!   shell where these are unset; stripping reproduces that.
//! - Workdirs live under `CARGO_TARGET_TMPDIR` (inside the repository) so a
//!   host that has already trusted this repository never sees a first-run
//!   trust prompt. Transcripts written under `~/.claude/projects` are
//!   best-effort deleted on teardown.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Json;
use serde_json::Value;

use delta_attribution::claude_format;
use delta_bootstrap::render_session_settings;
use delta_model::Role;
use delta_usecase::{pane_for, TmuxDriver};
use delta_wire::hooks::{
    PermissionRequestPayload, PermissionRequestResponse, PreToolUsePayload, SessionEndPayload,
    SessionStartPayload, StopPayload, UserPromptSubmitPayload,
};
use tmux_driver::Tmux;

/// How long to wait for a hook or transcript condition. Generous: a healthy
/// real turn completes in seconds; the deadline only bounds a broken run.
const WAIT_DEADLINE: Duration = Duration::from_secs(90);

/// Poll interval between probes.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Nested-session environment markers stripped from the spawned `claude`.
/// With any of these inherited (e.g. when this suite is itself driven from
/// inside a Claude Code session), `claude` treats itself as a child session
/// and does not persist its transcript JSONL.
const NESTED_CLAUDE_ENV: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_EFFORT",
    "AI_AGENT",
];

// --- Hook capture server ------------------------------------------------------

/// What the capture server answers to `PermissionRequest` POSTs.
#[derive(Clone, Copy)]
enum PermissionAnswer {
    /// Empty 200: the passthrough Delta sends when no browser decision landed.
    Passthrough,
    /// The decision envelope (`hookSpecificOutput.decision.behavior`).
    Decide { allow: bool },
}

/// Shared state of the in-process hook endpoint claude POSTs to.
struct Capture {
    /// `(path, body)` of every hook POST, in arrival order.
    events: Mutex<Vec<(String, Value)>>,
    /// `additionalContext` returned from `UserPromptSubmit`, when set.
    additional_context: Option<String>,
    permission_answer: PermissionAnswer,
}

impl Capture {
    fn count(&self, path: &str) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == path)
            .count()
    }

    fn bodies(&self, path: &str) -> Vec<Value> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == path)
            .map(|(_, b)| b.clone())
            .collect()
    }
}

async fn capture_hook(
    State(capture): State<Arc<Capture>>,
    axum::extract::Path(hook): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    capture
        .events
        .lock()
        .unwrap()
        .push((format!("/hooks/{hook}"), body));
    match hook.as_str() {
        "user-prompt-submit" => match &capture.additional_context {
            Some(context) => Json(serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "UserPromptSubmit",
                    "additionalContext": context,
                }
            }))
            .into_response(),
            None => ().into_response(),
        },
        "permission-request" => match capture.permission_answer {
            PermissionAnswer::Passthrough => ().into_response(),
            PermissionAnswer::Decide { allow } => {
                Json(PermissionRequestResponse::decided(allow)).into_response()
            }
        },
        _ => ().into_response(),
    }
}

/// Serve the capture router on an ephemeral loopback port.
async fn start_capture(capture: Arc<Capture>) -> (u16, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new()
        .route("/hooks/{hook}", post(capture_hook))
        .with_state(capture);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind hook capture listener");
    let port = listener.local_addr().expect("local addr").port();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve hook capture");
    });
    (port, server)
}

// --- A real claude session under Delta's spawn shape --------------------------

/// One spawned real-`claude` session: per-run tmux socket, Delta-rendered
/// settings, Delta's launch argv. Killed (with its whole tmux server) on drop.
struct ClaudeSession {
    tmux: Tmux,
    socket: String,
    pane: String,
    session_id: String,
    run_dir: PathBuf,
}

impl ClaudeSession {
    /// Launch `claude --settings <rendered> --session-id <uuid> <prompt>` in a
    /// fresh tmux session, exactly the argv Delta's spawn builds, with hooks
    /// pointing at the capture server on `hook_port`.
    async fn spawn(name: &str, hook_port: u16, prompt: &str) -> Self {
        let run_dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("real-claude-canary-{name}-{}", std::process::id()));
        let workdir = run_dir.join("workdir");
        std::fs::create_dir_all(&workdir).expect("create canary workdir");

        // Delta's production settings rendering: the hook URLs point at the
        // capture server instead of a delta-server, so the canary observes the
        // raw POSTs claude makes.
        let settings_path = run_dir.join("settings.json");
        std::fs::write(&settings_path, render_session_settings(hook_port))
            .expect("write rendered settings");

        let session_id = uuid::Uuid::now_v7().to_string();
        let claude_bin =
            std::env::var("DELTA_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_owned());

        // `env -u <marker> … <claude>` — see NESTED_CLAUDE_ENV.
        let mut command: Vec<String> = vec!["env".into()];
        for var in NESTED_CLAUDE_ENV {
            command.push("-u".into());
            command.push((*var).into());
        }
        command.extend([
            claude_bin,
            "--settings".into(),
            settings_path.to_string_lossy().into_owned(),
            "--session-id".into(),
            session_id.clone(),
            prompt.to_owned(),
        ]);

        let socket = format!("delta-canary-{name}-{}", std::process::id());
        let tmux = Tmux::new(socket.clone());
        let tmux_name = "canary";
        tmux.create_session(tmux_name, &workdir.to_string_lossy(), &command)
            .await
            .expect("tmux create_session");

        Self {
            tmux,
            socket,
            pane: pane_for(tmux_name),
            session_id,
            run_dir,
        }
    }

    /// Type `text` into the pane and submit it — Delta's own typing path
    /// (clear, literal text, delayed Enter).
    async fn send_line(&self, text: &str) {
        self.tmux
            .send_line(&self.pane, text)
            .await
            .expect("tmux send_line");
    }

    /// Press Escape in the pane (the TUI interrupt key).
    fn press_escape(&self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", &self.socket, "send-keys", "-t", &self.pane, "Escape"])
            .output();
    }

    /// The current pane content, for failure diagnostics.
    fn pane_content(&self) -> String {
        std::process::Command::new("tmux")
            .args(["-L", &self.socket, "capture-pane", "-p", "-t", &self.pane])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default()
    }
}

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .output();
        let _ = std::fs::remove_dir_all(&self.run_dir);
        // The per-socket config tmux-driver renders for `-f`.
        let _ = std::fs::remove_file(
            std::env::temp_dir().join(format!("delta-tmux-{}.conf", self.socket)),
        );
    }
}

// --- Helpers -------------------------------------------------------------------

/// Whether a usable `tmux` and `claude` are on `PATH`.
fn prerequisites_available() -> bool {
    let check = |bin: &str, arg: &str| {
        std::process::Command::new(bin)
            .arg(arg)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    };
    let claude_bin = std::env::var("DELTA_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_owned());
    check("tmux", "-V") && check(&claude_bin, "--version")
}

/// Poll until `condition` returns `Some`, or fail after [`WAIT_DEADLINE`] with
/// `what` (and the pane content) in the error.
async fn wait_for<T>(
    session: &ClaudeSession,
    what: &str,
    mut condition: impl FnMut() -> Option<T>,
) -> Result<T, String> {
    let deadline = Instant::now() + WAIT_DEADLINE;
    while Instant::now() < deadline {
        if let Some(value) = condition() {
            return Ok(value);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    Err(format!(
        "timed out waiting for {what}; pane:\n{}",
        session.pane_content()
    ))
}

/// The transcript path the session's hooks reported, once any hook carrying
/// one has arrived.
fn reported_transcript_path(capture: &Capture) -> Option<String> {
    capture
        .events
        .lock()
        .unwrap()
        .iter()
        .find_map(|(_, body)| body["transcript_path"].as_str().map(|s| s.to_owned()))
}

/// Raw JSONL lines of the transcript at `path` (empty when absent).
fn transcript_values(path: &str) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Parse every transcript line with Delta's own parser, keeping the lines it
/// yields messages for.
fn parsed_messages(path: &str) -> Vec<delta_usecase::TranscriptMessage> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| delta_transcript::parse_line(l).ok().flatten())
        .collect()
}

/// Per-attempt cleanup guard: stops the capture server and removes the
/// transcript claude wrote under `~/.claude/projects` when the attempt ends —
/// on the failure path (early `?` return) too, so a failed first attempt
/// never litters the host.
struct AttemptCleanup {
    capture: Arc<Capture>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for AttemptCleanup {
    fn drop(&mut self) {
        self.server.abort();
        remove_real_transcript(&self.capture);
    }
}

/// Best-effort cleanup of the transcript claude wrote under
/// `~/.claude/projects` for this canary session.
///
/// A `claude` that is still shutting down (the tmux kill in
/// [`ClaudeSession::drop`] is asynchronous from claude's point of view)
/// flushes its transcript once more on the way out, which can resurrect the
/// file right after a single removal. Retry briefly until the removal sticks.
fn remove_real_transcript(capture: &Capture) {
    let Some(path) = reported_transcript_path(capture) else {
        return;
    };
    let path = PathBuf::from(path);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let _ = std::fs::remove_file(&path);
        std::thread::sleep(Duration::from_millis(200));
        if !path.exists() || Instant::now() >= deadline {
            break;
        }
    }
    if let Some(parent) = path.parent() {
        // Only removes the per-workdir project dir when nothing else is
        // in it.
        let _ = std::fs::remove_dir(parent);
    }
}

/// Run `canary` and, when it fails, run it once more — exactly one retry per
/// canary, since a real model turn can flake in ways a scripted one cannot.
async fn with_one_retry<F, Fut>(name: &str, canary: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    if !prerequisites_available() {
        eprintln!("skipping {name}: tmux or claude is not available");
        return;
    }
    match canary().await {
        Ok(()) => {}
        Err(first) => {
            eprintln!("{name}: first attempt failed, retrying once: {first}");
            canary()
                .await
                .unwrap_or_else(|second| panic!("{name} failed twice: {second}"));
        }
    }
}

/// `Err(...)` with context unless `ok` holds.
fn ensure(ok: bool, context: &str, session: &ClaudeSession) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("{context}; pane:\n{}", session.pane_content()))
    }
}

// --- Canaries ------------------------------------------------------------------

#[tokio::test]
#[ignore = "drives the real claude CLI (consumes quota); run via make e2e-real"]
async fn prompt_turn_fires_hooks_and_streams_the_transcript() {
    with_one_retry("prompt_turn", || async {
        let capture = Arc::new(Capture {
            events: Mutex::new(Vec::new()),
            additional_context: Some("CANARY-INJECTED-CONTEXT".to_owned()),
            permission_answer: PermissionAnswer::Passthrough,
        });
        let (port, server) = start_capture(capture.clone()).await;
        let _cleanup = AttemptCleanup {
            capture: capture.clone(),
            server,
        };

        let prompt = "Reply with only the word: ok";
        let session = ClaudeSession::spawn("prompt-turn", port, prompt).await;

        // SessionStart (source=startup) arrives — via the `command` (curl)
        // hook, since claude does not deliver SessionStart to `http` hooks —
        // and deserializes with Delta's exact wire type.
        let start = wait_for(&session, "SessionStart hook", || {
            capture.bodies("/hooks/session-start").into_iter().next()
        })
        .await?;
        let start: SessionStartPayload =
            serde_json::from_value(start).map_err(|e| format!("SessionStart payload: {e}"))?;
        ensure(start.source == "startup", "SessionStart source", &session)?;
        ensure(
            start.session_id == session.session_id,
            "SessionStart carries the --session-id uuid",
            &session,
        )?;
        ensure(
            !start.transcript_path.is_empty() && !start.cwd.is_empty(),
            "SessionStart carries transcript_path and cwd",
            &session,
        )?;

        // UserPromptSubmit fires for the positional launch prompt.
        let submit = wait_for(&session, "UserPromptSubmit hook", || {
            capture
                .bodies("/hooks/user-prompt-submit")
                .into_iter()
                .next()
        })
        .await?;
        let submit: UserPromptSubmitPayload =
            serde_json::from_value(submit).map_err(|e| format!("UserPromptSubmit payload: {e}"))?;
        ensure(
            submit.prompt == prompt,
            "UserPromptSubmit carries the prompt verbatim",
            &session,
        )?;
        ensure(
            submit.session_id == session.session_id && !submit.transcript_path.is_empty(),
            "UserPromptSubmit carries session_id and transcript_path",
            &session,
        )?;

        // Stop fires when the turn completes, and deserializes.
        let stop = wait_for(&session, "Stop hook", || {
            capture.bodies("/hooks/stop").into_iter().next()
        })
        .await?;
        let stop: StopPayload =
            serde_json::from_value(stop).map_err(|e| format!("Stop payload: {e}"))?;
        ensure(
            stop.session_id == session.session_id,
            "Stop carries session_id",
            &session,
        )?;

        // The transcript at the hook-reported path streams the turn promptly:
        // a `role: user` line carrying the prompt and an assistant line with
        // text, both parsed by Delta's own parser, user before assistant.
        let transcript_path = reported_transcript_path(&capture).expect("transcript path");
        wait_for(&session, "user and assistant transcript lines", || {
            let messages = parsed_messages(&transcript_path);
            let user = messages.iter().position(|m| {
                m.role == Role::User
                    && m.flatten_text().is_some_and(|t| t.contains(prompt))
            })?;
            let assistant = messages.iter().position(|m| {
                m.role == Role::Assistant
                    && m.flatten_text().is_some_and(|t| !t.trim().is_empty())
            })?;
            (user < assistant).then_some(())
        })
        .await?;

        // The `additionalContext` envelope returned from UserPromptSubmit was
        // consumed: claude records the injected text in the transcript (as a
        // `hook_additional_context` attachment line on current versions — the
        // exact carrier is claude's business; that the marker text reached the
        // transcript at all is what proves the envelope shape still works).
        wait_for(&session, "injected additionalContext in the transcript", || {
            std::fs::read_to_string(&transcript_path)
                .unwrap_or_default()
                .contains("CANARY-INJECTED-CONTEXT")
                .then_some(())
        })
        .await?;

        // A turn with no permission dialog fires no PermissionRequest: the
        // hook is dialog-only (PreToolUse is the fires-for-every-call one).
        ensure(
            capture.count("/hooks/permission-request") == 0,
            "no PermissionRequest for a dialog-less turn",
            &session,
        )?;

        // `/exit`: the local-command caveat is recorded as an `isMeta` line —
        // harness-injected content Delta must classify as Role::Meta, not a
        // human turn — and SessionEnd fires.
        session.send_line("/exit").await;
        wait_for(&session, "SessionEnd hook", || {
            capture.bodies("/hooks/session-end").into_iter().next()
        })
        .await
        .and_then(|end| {
            serde_json::from_value::<SessionEndPayload>(end)
                .map(|_| ())
                .map_err(|e| format!("SessionEnd payload: {e}"))
        })?;
        wait_for(&session, "an isMeta caveat line parsed as Role::Meta", || {
            parsed_messages(&transcript_path)
                .iter()
                .any(|m| m.role == Role::Meta)
                .then_some(())
        })
        .await?;

        Ok(())
    })
    .await;
}

#[tokio::test]
#[ignore = "drives the real claude CLI (consumes quota); run via make e2e-real"]
async fn interrupting_a_turn_writes_the_marker_and_queued_prompts_dequeue() {
    // QUEUED-PROMPT FORMAT (claude 2.1.x): a prompt typed while a turn is in
    // flight is recorded as a uuid-less `{"type":"queue-operation",
    // "operation":"enqueue","content":…}` line at submit time, and on dequeue
    // replayed as a plain `type:"user"` line (promptSource "queued") that
    // fires its own `UserPromptSubmit` hook — i.e. it flows through the same
    // path as any TUI-typed prompt. This canary pins that reality; fake-claude
    // re-enacts it (`enqueue_prompt`/`dequeue_prompt` steps). Older claude
    // versions wrote a `queued_command` attachment line instead, which
    // `delta-transcript` still parses as legacy-format compatibility for
    // transcripts recorded back then (see the queued-prompt drift note in
    // docs/guides/development.md).
    with_one_retry("interrupt_and_queued", || async {
        let capture = Arc::new(Capture {
            events: Mutex::new(Vec::new()),
            additional_context: None,
            permission_answer: PermissionAnswer::Passthrough,
        });
        let (port, server) = start_capture(capture.clone()).await;
        let _cleanup = AttemptCleanup {
            capture: capture.clone(),
            server,
        };

        // A turn long enough to still be streaming when the queued prompt and
        // the Escape land (pure token output, no tools).
        let prompt = "Count from 1 to 100 out loud, one number per line. Do not use any tools.";
        let session = ClaudeSession::spawn("interrupt", port, prompt).await;

        wait_for(&session, "UserPromptSubmit hook", || {
            (capture.count("/hooks/user-prompt-submit") >= 1).then_some(())
        })
        .await?;
        // Let the turn actually get in flight before typing into the busy pane.
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Type a prompt while the turn is busy: claude queues it, recording a
        // uuid-less queue-operation enqueue line carrying the text.
        let queued = "Reply with only the word: ok";
        session.send_line(queued).await;
        let transcript_path = wait_for(&session, "hook-reported transcript path", || {
            reported_transcript_path(&capture)
        })
        .await?;
        wait_for(&session, "queue-operation enqueue line", || {
            transcript_values(&transcript_path)
                .iter()
                .any(|v| {
                    v["type"] == "queue-operation"
                        && v["operation"] == "enqueue"
                        && v["content"].as_str().is_some_and(|c| c.contains(queued))
                })
                .then_some(())
        })
        .await?;
        ensure(
            capture.count("/hooks/stop") == 0,
            "the long turn is still in flight",
            &session,
        )?;

        // Escape interrupts the in-flight turn: the marker is written as a
        // `role: user` line whose text Delta's own predicate recognizes, and
        // NO Stop hook fires for the aborted turn. The press is retried with a
        // settle beat, checking for the marker *before* each press — once the
        // interrupt lands, the queued prompt dequeues immediately, and a
        // blind extra Escape could interrupt that turn too.
        let mut stops_before_marker = None;
        for _ in 0..20 {
            let marker_present = parsed_messages(&transcript_path).iter().any(|m| {
                m.role == Role::User
                    && m.flatten_text()
                        .is_some_and(|t| claude_format::is_interrupt_marker(t.trim()))
            });
            if marker_present {
                stops_before_marker = Some(capture.count("/hooks/stop"));
                break;
            }
            session.press_escape();
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
        let stops_before_marker = stops_before_marker.ok_or_else(|| {
            format!(
                "interrupt marker never appeared; pane:\n{}",
                session.pane_content()
            )
        })?;
        ensure(
            stops_before_marker == 0,
            "no Stop fired for the interrupted turn",
            &session,
        )?;

        // The queued prompt dequeues (the interrupt frees the turn): it is
        // replayed as a plain user line and fires its own UserPromptSubmit.
        // Both happen at dequeue time, before its replayed turn produces
        // anything, so they are immune to a stray extra Escape landing on
        // that turn. Its completion is not awaited — Stop-on-completion is
        // already pinned by the prompt-turn canary.
        wait_for(&session, "dequeued user line for the queued prompt", || {
            parsed_messages(&transcript_path)
                .iter()
                .any(|m| {
                    m.role == Role::User
                        && m.flatten_text().is_some_and(|t| t.contains(queued))
                })
                .then_some(())
        })
        .await?;
        wait_for(&session, "UserPromptSubmit for the dequeued prompt", || {
            (capture.count("/hooks/user-prompt-submit") >= 2).then_some(())
        })
        .await?;

        Ok(())
    })
    .await;
}

#[tokio::test]
#[ignore = "drives the real claude CLI (consumes quota); run via make e2e-real"]
async fn permission_dialog_fires_the_hook_and_the_allow_decision_is_honored() {
    with_one_retry("permission", || async {
        let capture = Arc::new(Capture {
            events: Mutex::new(Vec::new()),
            additional_context: None,
            permission_answer: PermissionAnswer::Decide { allow: true },
        });
        let (port, server) = start_capture(capture.clone()).await;
        let _cleanup = AttemptCleanup {
            capture: capture.clone(),
            server,
        };

        // `rm` is never auto-approved in default permission mode, so this tool
        // call reliably raises an interactive permission dialog. The file does
        // not exist; `rm -f` on it is a no-op even when allowed.
        let prompt = "Use the Bash tool to run exactly this command: rm -f canary-scratch.txt\n\
                      Then reply with the single word: done";
        let session = ClaudeSession::spawn("permission", port, prompt).await;

        // PreToolUse fires for the imminent call and carries the tool_use_id
        // Delta correlates the eventual tool_result with.
        let pre = wait_for(&session, "PreToolUse hook", || {
            capture.bodies("/hooks/pre-tool-use").into_iter().next()
        })
        .await?;
        let pre: PreToolUsePayload =
            serde_json::from_value(pre).map_err(|e| format!("PreToolUse payload: {e}"))?;
        ensure(pre.tool_name == "Bash", "PreToolUse tool_name", &session)?;
        ensure(
            !pre.tool_use_id.is_empty(),
            "PreToolUse carries tool_use_id",
            &session,
        )?;

        // PermissionRequest fires because a dialog would appear. It carries
        // tool_name/tool_input but — load-bearing for Delta's row-ownership
        // design, which correlates by (session, tool_name, tool_input) — NO
        // tool_use_id.
        let raw = wait_for(&session, "PermissionRequest hook", || {
            capture
                .bodies("/hooks/permission-request")
                .into_iter()
                .next()
        })
        .await?;
        ensure(
            raw.get("tool_use_id").is_none(),
            "PermissionRequest has no tool_use_id key",
            &session,
        )?;
        let permission: PermissionRequestPayload =
            serde_json::from_value(raw).map_err(|e| format!("PermissionRequest payload: {e}"))?;
        ensure(
            permission.tool_name == "Bash",
            "PermissionRequest tool_name",
            &session,
        )?;
        ensure(
            permission.tool_input["command"]
                .as_str()
                .is_some_and(|c| c.contains("rm -f canary-scratch.txt")),
            "PermissionRequest tool_input carries the command",
            &session,
        )?;

        // The allow decision envelope was honored: the tool actually ran (its
        // tool_result line lands with the PreToolUse id and no error) and the
        // turn completes.
        let transcript_path = reported_transcript_path(&capture).expect("transcript path");
        wait_for(&session, "tool_result line for the allowed call", || {
            transcript_values(&transcript_path)
                .iter()
                .any(|v| {
                    v["type"] == "user"
                        && v["message"]["content"]
                            .as_array()
                            .is_some_and(|blocks| {
                                blocks.iter().any(|b| {
                                    b["type"] == "tool_result"
                                        && b["tool_use_id"] == pre.tool_use_id.as_str()
                                        && b["is_error"] == false
                                })
                            })
                })
                .then_some(())
        })
        .await?;
        wait_for(&session, "Stop hook after the allowed tool", || {
            (capture.count("/hooks/stop") >= 1).then_some(())
        })
        .await?;

        Ok(())
    })
    .await;
}
