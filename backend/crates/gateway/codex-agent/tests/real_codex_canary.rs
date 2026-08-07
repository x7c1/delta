//! Real-`codex` canaries: the C4 end-to-end validation of the R1–R3 wire
//! reconciliation, run against the REAL `codex app-server` binary (not the
//! `fake-codex` re-enactment every other test drives).
//!
//! Two canaries live here, both `#[ignore]` so neither runs under a bare
//! `cargo test` or in CI (see "Why both are `#[ignore]`" below):
//!
//! 1. [`real_codex_completes_a_safe_turn_end_to_end`] — drives Delta's REAL
//!    [`CodexAppServerAdapter`] (stood up by the REAL [`CodexAdapterFactory`],
//!    which spawns `codex app-server` and runs the real `initialize`
//!    handshake) through one safe turn and asserts the neutral
//!    [`AgentEvent`]s Delta produces: `TurnStarted`, an assistant reply
//!    (`AssistantMessage` / `AssistantDelta`), and `TurnCompleted(Completed)`.
//!    The prompt asks for a one-word reply and forbids tools/commands, so the
//!    turn runs NO tools → raises NO approval → executes nothing: safe and
//!    cheap. Needs the real binary + auth + network; consumes a tiny amount of
//!    the authenticated user's Codex quota.
//!
//! 2. [`vendored_schema_matches_the_real_generator`] — regenerates the schema
//!    with `codex app-server generate-json-schema` (a static dump: NO auth, NO
//!    network) and compares it against the vendored ground truth under
//!    `vendor/app-server-schema/`, failing loudly if the real generator's
//!    output diverges from the pinned [`VENDORED_CODEX_VERSION`]. This is the
//!    guard that a `codex` upgrade which moves the protocol is caught rather
//!    than silently drifting the `fake-codex` re-enactment green.
//!
//! ## Why both are `#[ignore]`
//!
//! Delta's existing real-binary lane (the real-`claude` canaries in
//! `delta-server/tests/real_claude_canary.rs`) is `#[ignore]` for exactly this
//! reason: CI runners have no provider binary (and no auth), so a
//! presence-gated test would only ever skip there while making the local
//! `make check` gate non-hermetic — its result would depend on which `codex`
//! version happens to be installed on the developer's host. Keeping BOTH
//! canaries `#[ignore]` keeps the normal gate hermetic and offline, and makes
//! the real-binary lane a single explicit opt-in, mirroring `make e2e-real`.
//!
//! The drift canary needs only the binary (no auth/network), so it *could*
//! have been presence-gated; it is `#[ignore]` anyway, for the hermeticity
//! reason above and so one command runs the whole real-`codex` lane.
//!
//! ## Running them
//!
//! ```text
//! make e2e-real-codex
//! # or directly:
//! CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/cc \
//!   cargo test -p codex-agent --test real_codex_canary -- --ignored --nocapture
//! ```
//!
//! `DELTA_CODEX_BIN` overrides the binary (default: `codex` on PATH), matching
//! `DELTA_CLAUDE_BIN` in the real-`claude` lane.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use codex_agent::schema::{
    COMBINED_SERVER_REQUEST_SCHEMA_RELATIVE_PATH, V2_COMBINED_SCHEMA_RELATIVE_PATH,
    VENDORED_CODEX_VERSION,
};
use codex_agent::{CodexAdapterFactory, CodexLaunchConfig};
use delta_usecase::{
    AgentAdapter, AgentAdapterFactory, AgentEvent, AgentEventStream, ContentSourceRequest,
    LaunchRequest, SendRequest, SessionId, ThreadId, TurnStatus,
};
use serde_json::Value;
use tokio::time::timeout;

/// The `codex` binary the canaries drive: `DELTA_CODEX_BIN` if set, else the
/// `codex` on `PATH` (mirrors `DELTA_CLAUDE_BIN` in the real-`claude` lane).
fn codex_bin() -> String {
    std::env::var("DELTA_CODEX_BIN").unwrap_or_else(|_| "codex".to_owned())
}

// --- Canary 1: a real turn, end to end -------------------------------------

/// A generous per-event bound: a healthy real turn completes in ~10–40s; the
/// deadline only bounds a broken run so a hang fails instead of blocking forever.
const TURN_TIMEOUT: Duration = Duration::from_secs(120);

/// A prompt that elicits a short assistant reply and runs NO tools/commands:
/// no command → no approval → nothing executes, so the turn is safe and cheap.
const SAFE_PROMPT: &str =
    "Reply with exactly the word: hello. Do not use any tools or run any commands.";

/// A unique temp working directory, removed on drop. The real turn is given a
/// throwaway `cwd` so it never touches the repository.
struct TempCwd {
    path: PathBuf,
}

impl TempCwd {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "delta-real-codex-canary-{}-{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&path).expect("create temp cwd");
        Self { path }
    }

    fn as_str(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for TempCwd {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).ok();
    }
}

/// Receive events from `stream` until `stop` returns true for one (inclusive),
/// the stream closes, or the per-event timeout fires (a hang — itself a
/// failure). Returns everything collected.
async fn collect_until<F>(stream: &mut AgentEventStream, stop: F) -> Vec<AgentEvent>
where
    F: Fn(&AgentEvent) -> bool,
{
    let mut events = Vec::new();
    loop {
        match timeout(TURN_TIMEOUT, stream.recv()).await {
            Ok(Some(event)) => {
                let done = stop(&event);
                events.push(event);
                if done {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => panic!(
                "timed out after {:?} waiting for the real Codex turn to complete; \
                 collected so far: {events:?}",
                TURN_TIMEOUT
            ),
        }
    }
    events
}

/// Drive Delta's real adapter against the real `codex app-server` through one
/// safe turn and assert the neutral events Delta produces.
///
/// This is the payoff proof of the R1–R3 reconciliation against ground truth:
/// the whole path — real spawn, real `initialize` handshake, real
/// `thread/start` / `turn/start`, the real pushed `turn/*` / `item/*`
/// notifications, and Delta's `translate` layer — must yield a completed turn
/// carrying an assistant reply, exactly as it does against `fake-codex`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives the real codex app-server: needs the binary + auth + network, consumes Codex quota"]
async fn real_codex_completes_a_safe_turn_end_to_end() {
    let cwd = TempCwd::new();

    // The REAL composition path: the factory spawns `codex app-server` and runs
    // the real `initialize` handshake, handing back the adapter over the same
    // `Arc<dyn AgentAdapter>` trait object the core holds.
    let config = CodexLaunchConfig {
        codex_bin: codex_bin(),
        args: vec!["app-server".to_owned()],
        env: vec![],
    };
    let factory = CodexAdapterFactory::new(config);
    let adapter: std::sync::Arc<dyn AgentAdapter> = factory
        .connect()
        .await
        .expect("connect to the real codex app-server (spawn + initialize)");

    // Launch a thread with a throwaway cwd and no auto-prompt, then subscribe
    // BEFORE sending so no event is missed.
    let handle = adapter
        .launch(LaunchRequest {
            session_id: "01920000-0000-7000-8000-0000000000c4".to_owned(),
            workdir: cwd.as_str(),
            launch_options: Vec::new(),
            first_prompt: None,
        })
        .await
        .expect("launch a real Codex thread");
    let mut stream = adapter.events(&handle);

    // The real `thread/start` response announces the model the server resolved
    // for the thread, and the adapter carries it onto every message the session
    // folds. Asserting it here — against the real binary — is what keeps the
    // vendored claim ("`model` is a required top-level field of
    // `ThreadStartResponse`") honest: if a future codex release moves or drops
    // it, sessions would silently go back to reporting no model, and only this
    // canary would notice. The value itself is not pinned (which model the
    // account resolves to is not Delta's business); that it is reported at all
    // is.
    let mut content = adapter.content_source(
        &handle,
        ContentSourceRequest {
            session_id: SessionId::from("01920000-0000-7000-8000-0000000000c4"),
            main_thread: ThreadId(1),
            seed_seq: 0,
            cwd: cwd.as_str(),
            git_branch: None,
        },
    );
    let (probe, _) = content.ingest(&AgentEvent::AssistantMessage {
        provider_item_id: "probe".to_owned(),
        text: "probe".to_owned(),
        at_ms: None,
    });
    let reported_model = probe[0].model.clone();
    assert!(
        reported_model
            .as_deref()
            .is_some_and(|model| !model.is_empty()),
        "the real thread/start response must report the resolved model, got {reported_model:?}"
    );

    // Drain the opening SessionStarted so the turn assertions start clean.
    match timeout(TURN_TIMEOUT, stream.recv()).await {
        Ok(Some(AgentEvent::SessionStarted { .. })) => {}
        other => panic!("expected the opening SessionStarted, got {other:?}"),
    }

    adapter
        .send(
            &handle,
            SendRequest {
                text: SAFE_PROMPT.to_owned(),
            },
        )
        .await
        .expect("start a real turn");

    let events = collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::TurnCompleted { .. })
    })
    .await;

    // The turn started.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::TurnStarted { .. })),
        "expected TurnStarted from the real turn, got {events:?}"
    );

    // An assistant reply arrived — as a completed message and/or streaming
    // deltas. Assertions stay robust to model wording: an assistant message
    // must arrive; text is only checked leniently (contains "hello", case
    // insensitive) since the model's exact wording is not deterministic.
    let assistant_texts: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::AssistantMessage { text, .. } => Some(text.as_str()),
            AgentEvent::AssistantDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !assistant_texts.is_empty(),
        "expected an assistant reply (AssistantMessage/AssistantDelta), got {events:?}"
    );
    let joined = assistant_texts.join("").to_lowercase();
    assert!(
        joined.contains("hello"),
        "the assistant reply should contain the requested word `hello` (lenient); \
         got {assistant_texts:?}"
    );

    // No tool ran and no approval was raised: the prompt forbids commands, so
    // the turn must not have executed anything.
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolStarted { .. })),
        "the no-command prompt must run no tools, got {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AgentEvent::PermissionRequested { .. })),
        "the no-command prompt must raise no approval, got {events:?}"
    );

    // The turn completed cleanly.
    let completed = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                AgentEvent::TurnCompleted {
                    status: TurnStatus::Completed
                }
            )
        })
        .count();
    assert_eq!(
        completed, 1,
        "expected exactly one TurnCompleted(Completed) from the real turn, got {events:?}"
    );

    // Close the session cleanly.
    adapter.close(&handle).await.expect("close the session");
}

// --- Canary 2: schema drift detection --------------------------------------

/// The combined-schema documents Delta vendors and reconciles against, by
/// crate-relative path.
const COMBINED_SCHEMA_PATHS: &[&str] = &[
    V2_COMBINED_SCHEMA_RELATIVE_PATH,
    COMBINED_SERVER_REQUEST_SCHEMA_RELATIVE_PATH,
];

/// The v2 combined-schema definitions Delta's `wire` / `translate` / adapter
/// layers reconciled against (R1 envelope, R3 item content): the client-request
/// param/response types Delta sends and the pushed notifications it parses.
const RELIED_ON_V2_DEFINITIONS: &[&str] = &[
    "ThreadStartParams",
    "ThreadStartResponse",
    "ThreadResumeParams",
    "ThreadResumeResponse",
    "TurnStartParams",
    "TurnStartResponse",
    "TurnInterruptParams",
    "TurnInterruptResponse",
    "TurnStartedNotification",
    "TurnCompletedNotification",
    "ItemStartedNotification",
    "ItemCompletedNotification",
    "AgentMessageDeltaNotification",
];

/// The non-versioned combined-schema definitions Delta's approval fan-out (R2)
/// reconciled against: the server → client request registry and its approval
/// param/response types.
const RELIED_ON_SERVER_REQUEST_DEFINITIONS: &[&str] = &[
    "ServerRequest",
    "CommandExecutionRequestApprovalParams",
    "CommandExecutionRequestApprovalResponse",
    "FileChangeRequestApprovalParams",
    "FileChangeRequestApprovalResponse",
    "PermissionsRequestApprovalParams",
    "PermissionsRequestApprovalResponse",
];

/// The approval methods Delta's fan-out routes, which must remain in the
/// generated `ServerRequest` registry.
const APPROVAL_METHODS: &[&str] = &[
    "item/commandExecution/requestApproval",
    "item/fileChange/requestApproval",
    "item/permissions/requestApproval",
];

/// Read and parse a vendored combined schema by crate-relative path.
fn read_vendored(relative: &str) -> Value {
    let path = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), relative);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("vendored schema missing at {path}: {err}"));
    serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("vendored schema {path} is not JSON: {err}"))
}

/// The `definitions` object of a parsed schema document.
fn definitions(doc: &Value) -> &serde_json::Map<String, Value> {
    doc.get("definitions")
        .and_then(Value::as_object)
        .expect("schema document has a `definitions` object")
}

/// Regenerate the schema and assert the vendored ground truth still matches the
/// real generator's output — i.e. `codex` has not drifted from the pinned
/// [`VENDORED_CODEX_VERSION`].
///
/// The comparison is deliberately *not* a raw byte-diff (which would break on
/// formatting). It works at three levels, cheapest and loudest first:
///
/// 1. the installed `codex --version` still reports the pinned version;
/// 2. each combined document's `definitions` **key set** is unchanged (a type
///    added / removed / renamed is caught here) — order-independent;
/// 3. every definition Delta actually reconciled against is **value-equal**
///    between vendored and generated (a field-level change to a type Delta
///    parses is caught here) — order-independent for objects, and stable for
///    the arrays a single pinned version emits.
///
/// A failure means `codex` moved the protocol: re-vendor `vendor/app-server-schema/`
/// and bump `VENDORED_CODEX_VERSION` in the same change.
#[tokio::test]
#[ignore = "regenerates the schema with the real codex binary (no auth/network); excluded from the hermetic gate"]
async fn vendored_schema_matches_the_real_generator() {
    // 1. Version pin: the loudest, most human-readable drift signal.
    let version_output = Command::new(codex_bin())
        .arg("--version")
        .output()
        .expect("run `codex --version`");
    assert!(
        version_output.status.success(),
        "`codex --version` failed: {}",
        String::from_utf8_lossy(&version_output.stderr)
    );
    let version = String::from_utf8_lossy(&version_output.stdout);
    assert!(
        version.contains(VENDORED_CODEX_VERSION),
        "installed codex reports `{}` but the vendored schema is pinned to `{VENDORED_CODEX_VERSION}`; \
         re-vendor vendor/app-server-schema/ and bump VENDORED_CODEX_VERSION",
        version.trim()
    );

    // Regenerate into a throwaway dir (a static dump — no auth, no network).
    let out = TempCwd::new();
    let gen = Command::new(codex_bin())
        .args(["app-server", "generate-json-schema", "--out"])
        .arg(&out.path)
        .output()
        .expect("run `codex app-server generate-json-schema`");
    assert!(
        gen.status.success(),
        "generate-json-schema failed: {}",
        String::from_utf8_lossy(&gen.stderr)
    );

    for relative in COMBINED_SCHEMA_PATHS {
        // The generator writes the combined docs under their fixed basenames.
        let basename = relative
            .rsplit('/')
            .next()
            .expect("a schema path has a basename");
        let generated_path = out.path.join(basename);
        let generated_raw = std::fs::read_to_string(&generated_path).unwrap_or_else(|err| {
            panic!("generator did not emit {}: {err}", generated_path.display())
        });
        let generated: Value = serde_json::from_str(&generated_raw)
            .unwrap_or_else(|err| panic!("generated {basename} is not JSON: {err}"));
        let vendored = read_vendored(relative);

        // 2. definitions key set unchanged (order-independent).
        let gen_defs = definitions(&generated);
        let ven_defs = definitions(&vendored);
        let gen_keys: std::collections::BTreeSet<&String> = gen_defs.keys().collect();
        let ven_keys: std::collections::BTreeSet<&String> = ven_defs.keys().collect();
        let added: Vec<&&String> = gen_keys.difference(&ven_keys).collect();
        let removed: Vec<&&String> = ven_keys.difference(&gen_keys).collect();
        assert!(
            added.is_empty() && removed.is_empty(),
            "schema drift in {basename}: definition set changed \
             (added by generator: {added:?}, missing from generator: {removed:?}); \
             re-vendor and bump VENDORED_CODEX_VERSION"
        );

        // 3. every relied-on definition is value-equal (order-independent).
        let relied_on = if basename.contains(".v2.") {
            RELIED_ON_V2_DEFINITIONS
        } else {
            RELIED_ON_SERVER_REQUEST_DEFINITIONS
        };
        for name in relied_on {
            let g = gen_defs.get(*name).unwrap_or_else(|| {
                panic!("generated {basename} lost relied-on definition `{name}`")
            });
            let v = ven_defs.get(*name).unwrap_or_else(|| {
                panic!("vendored {basename} lacks relied-on definition `{name}`")
            });
            assert_eq!(
                g, v,
                "schema drift in {basename}: relied-on definition `{name}` changed shape; \
                 re-vendor and bump VENDORED_CODEX_VERSION"
            );
        }
    }

    // The approval methods Delta routes must remain in the generated registry.
    let generated_server_request = {
        let basename = COMBINED_SERVER_REQUEST_SCHEMA_RELATIVE_PATH
            .rsplit('/')
            .next()
            .unwrap();
        std::fs::read_to_string(out.path.join(basename)).expect("read generated server-request doc")
    };
    for method in APPROVAL_METHODS {
        assert!(
            generated_server_request.contains(method),
            "the real generator's server-request registry no longer carries approval method `{method}`"
        );
    }
}
