//! Real-`codex` canaries: the C4 end-to-end validation of the R1–R3 wire
//! reconciliation, run against the REAL `codex app-server` binary (not the
//! `fake-codex` re-enactment every other test drives).
//!
//! Four canaries live here, all `#[ignore]` so none runs under a bare
//! `cargo test` or in CI (see "Why they are all `#[ignore]`" below):
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
//! 2. [`real_thread_start_reports_the_metadata_delta_stamps_on_messages`] —
//!    starts one thread in this checkout (NO turn, so no model quota) and pins
//!    what `thread/start` really reports: the top-level `model` is present, the
//!    `cwd` is echoed back verbatim, and `thread.gitInfo` is **null** despite the
//!    schema declaring it — which is why Delta observes its launch directory's
//!    branch itself. Needs the real binary + auth; no turn is run.
//!
//! 3. [`real_thread_start_honors_the_worktree_git_grant`] — starts two threads
//!    (NO turn, so no model quota) and pins that the **dotted** config key
//!    Delta injects for a worktree session,
//!    `sandbox_workspace_write.writable_roots`, really reaches the thread's
//!    effective sandbox policy. The vendored schema types `config` as a
//!    free-form object and so cannot say this; without the canary, an ignored
//!    key would leave every unit test green and every `git add` prompting.
//!    Needs the real binary + auth.
//!
//! 4. [`vendored_schema_matches_the_real_generator`] — regenerates the schema
//!    with `codex app-server generate-json-schema` (a static dump: NO auth, NO
//!    network) and compares it against the vendored ground truth under
//!    `vendor/app-server-schema/`, failing loudly if the real generator's
//!    output diverges from the pinned [`VENDORED_CODEX_VERSION`]. This is the
//!    guard that a `codex` upgrade which moves the protocol is caught rather
//!    than silently drifting the `fake-codex` re-enactment green.
//!
//! ## Why they are all `#[ignore]`
//!
//! Delta's existing real-binary lane (the real-`claude` canaries in
//! `delta-server/tests/real_claude_canary.rs`) is `#[ignore]` for exactly this
//! reason: CI runners have no provider binary (and no auth), so a
//! presence-gated test would only ever skip there while making the local
//! `make check` gate non-hermetic — its result would depend on which `codex`
//! version happens to be installed on the developer's host. Keeping EVERY
//! canary `#[ignore]` keeps the normal gate hermetic and offline, and makes
//! the real-binary lane a single explicit opt-in, mirroring `make e2e-real`.
//!
//! The drift canary needs only the binary (no auth/network), so it *could*
//! have been presence-gated; it is `#[ignore]` anyway, for the hermeticity
//! reason above and so one command runs the whole real-`codex` lane.
//!
//! ## Being `#[ignore]`d, they must be run deliberately
//!
//! Nothing in `make check` or CI runs these, so an assumption about the wire can
//! be wrong for as long as nobody types the command. That is not hypothetical:
//! Delta once sourced a message's `git_branch` from `thread.gitInfo` — a field
//! the vendored schema declares and documents — and shipped permanently blank
//! branches, because the real server returns it null and no one had run the
//! canary that would have said so.
//!
//! So: **run `make e2e-real-codex` whenever a change depends on what the real
//! server sends or returns**, not only when `codex` is upgraded. A schema field's
//! existence is not evidence that a given response populates it; this lane is how
//! that difference gets checked.
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

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use codex_agent::schema::{
    COMBINED_SERVER_REQUEST_SCHEMA_RELATIVE_PATH, V2_COMBINED_SCHEMA_RELATIVE_PATH,
    VENDORED_CODEX_VERSION,
};
use codex_agent::{
    thread_start_params, AppServerConnection, CodexAdapterFactory, CodexLaunchConfig,
};
use delta_usecase::{
    AgentAdapter, AgentAdapterFactory, AgentEvent, AgentEventStream, ContentSourceRequest,
    LaunchOptionSpec, LaunchRequest, SendRequest, SessionId, ThreadId, TurnStatus,
};
use serde_json::{json, Value};
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
            worktree_repo_root: None,
        })
        .await
        .expect("launch a real Codex thread");
    let mut stream = adapter.events(&handle);

    // The metadata plumbing works against the real server: what the adapter
    // recorded from the real `thread/start` response reaches a folded message.
    // (Canary 3 checks the wire fields themselves; this checks that they travel
    // the whole adapter -> content-source path.) The model value is not pinned —
    // which model the account resolves to is not Delta's business — only that it
    // is reported at all.
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

// --- Canary 2: the thread-metadata wire fields -----------------------------

/// The `thread/start` response fields Delta reads a session's message metadata
/// from behave as Delta assumes — checked against the live binary rather than
/// inferred from the vendored schema.
///
/// Delta stamps three facts on every Codex message, from two different sources,
/// and each rests on a claim about this one response:
///
/// 1. **`model`** — a required top-level string, and the only truthful source
///    for what is running, since the server reconciles the launch option, the
///    user's config and its own default. Asserted present.
/// 2. **`cwd`** — echoed back verbatim. Delta does NOT read the message `cwd`
///    from here (it keeps its own recorded launch directory, the string its
///    repo-root / requested-workdir columns are stored against), but the two are
///    supposed to describe the same place. Asserting the echo is verbatim keeps
///    that decision a preference rather than a papered-over divergence.
/// 3. **`thread.gitInfo`** — asserted **null**, which is the opposite of what
///    the schema suggests.
///
/// That third assertion is the interesting one. `Thread.gitInfo` is declared as
/// `GitInfo | null` and documented as "Optional Git metadata captured when the
/// thread was created", which reads like a populated field. It is not: as of
/// `codex-cli 0.144.4` the real server returns `gitInfo: null` on `thread/start`
/// even when `cwd` is a git working tree on a named branch. The declared value
/// is evidently materialised on some other read path, not this response.
///
/// Delta briefly sourced a message's `git_branch` from it and shipped sessions
/// with a permanently empty branch as a result. It now observes its own launch
/// directory's branch instead, and this canary pins the reason: if a future
/// release starts populating `gitInfo` here, this fails and someone can revisit
/// the choice deliberately — rather than the schema being re-read as evidence a
/// second time.
///
/// No turn is started, so unlike canary 1 this consumes no model quota — it is
/// one `thread/start` against the real server.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives the real codex app-server: needs the binary + auth, consumes no model quota"]
async fn real_thread_start_reports_the_metadata_delta_stamps_on_messages() {
    // Start the thread in this checkout: a real git working tree, so the server
    // would have real git metadata to report if this response reported any.
    // Nothing is executed in it — only a thread record is created.
    let repo_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let git_branch = git_branch_of(&repo_dir);
    let cwd = repo_dir.to_string_lossy().into_owned();

    let conn = AppServerConnection::spawn(&CodexLaunchConfig {
        codex_bin: codex_bin(),
        args: vec!["app-server".to_owned()],
        env: vec![],
    })
    .expect("spawn the real codex app-server");
    conn.initialize(json!({
        "clientInfo": { "name": "delta", "version": env!("CARGO_PKG_VERSION") }
    }))
    .await
    .expect("initialize handshake");

    let started = conn
        .start_thread(None, Some(json!({ "cwd": cwd })))
        .await
        .expect("start a real thread");
    let result = &started.result;

    // 1. The model is reported. The value is not pinned — which model the
    //    account resolves to is not Delta's business — only that it is there.
    let model = result.get("model").and_then(Value::as_str);
    assert!(
        model.is_some_and(|model| !model.is_empty()),
        "thread/start must report the resolved model at the top level, got {result}"
    );

    // 2. The cwd comes back verbatim, so Delta's recorded launch directory and
    //    the thread's own cwd describe the same place with the same spelling.
    assert_eq!(
        result.get("cwd").and_then(Value::as_str),
        Some(cwd.as_str()),
        "thread/start must echo the cwd it was given, got {result}"
    );

    // 3. `gitInfo` is null even though `cwd` is a git working tree — the pin that
    //    keeps Delta observing the branch itself. Only meaningful when git agrees
    //    this directory really is on a branch, so the "the server had something
    //    to report and still reported nothing" reading holds.
    let git_info = result
        .get("thread")
        .and_then(|thread| thread.get("gitInfo"));
    match git_branch {
        Some(branch) => assert!(
            matches!(git_info, None | Some(Value::Null)),
            "thread/start is expected to report NO git metadata (as of \
             codex-cli {VENDORED_CODEX_VERSION}), yet it reported {git_info:?} while git says \
             this cwd is on `{branch}`. If the server now populates gitInfo, Delta \
             could source a message's git_branch from it instead of observing the \
             launch directory itself — revisit deliberately. Full response: {result}"
        ),
        // No git in PATH, or this checkout is not a working tree (a tarball, a
        // detached HEAD): the server having nothing to report would be correct,
        // so the pin says nothing.
        None => eprintln!(
            "skipped the gitInfo pin: git reported no branch for {cwd}; \
             the server said {git_info:?}"
        ),
    }
}

/// The branch `git` reports for `dir`, or `None` when git is unavailable, `dir`
/// is not a working tree, or HEAD is detached — the same three cases in which
/// Delta expects no branch to be reported.
fn git_branch_of(dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!branch.is_empty() && branch != "HEAD").then_some(branch)
}

// --- Canary 3: the worktree git-directory grant ----------------------------

/// The real server honours the sandbox grant Delta injects for a worktree
/// session — the claim the whole feature rests on, and one only the live binary
/// can settle.
///
/// Delta grants a worktree session's real git directory (the source
/// repository's `.git`, which a linked worktree's own `.git` only points at) by
/// putting it on the **dotted** config key
/// `sandbox_workspace_write.writable_roots` in `thread/start`'s free-form
/// `config`. The vendored schema types that field as an object with arbitrary
/// keys and stops there: it says nothing about whether a dotted key is applied
/// at the leaf the way the CLI's `-c` flag applies it, or whether it is merely
/// stored as an oddly-named unknown key and ignored. If it were ignored, every
/// unit test here would still pass while users kept getting an approval prompt
/// on every `git add` — the exact failure this feature exists to remove.
///
/// So the assertion reads the **effective** policy back off the response
/// (`sandbox.writableRoots`, which the server reports for the thread it just
/// configured) and requires the granted path to be in it.
///
/// The params come from `thread_start_params` — the same builder `launch` uses
/// — so what is pinned is the request production really sends, not a shape
/// re-spelled in a test. `sandbox` is forced to `workspace-write` so the
/// effective policy carries writable roots at all, whatever the developer's own
/// default is.
///
/// Also worth reading in the output: the baseline roots (a start with no grant)
/// against the granted ones. As of `codex-cli 0.144.4` the leaf override
/// **replaces** the user's global `writable_roots` rather than unioning with it
/// — accepted deliberately (see the adapter's module docs), and printed here so
/// the trade-off stays visible rather than folklore.
///
/// No turn is started, so this consumes no model quota — two `thread/start`
/// calls against the real server.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "drives the real codex app-server: needs the binary + auth, consumes no model quota"]
async fn real_thread_start_honors_the_worktree_git_grant() {
    // A throwaway cwd stands in for the worktree and a throwaway directory for
    // the repository it would have been cut from: the grant is a path the
    // sandbox is told about, and nothing is executed in either.
    let worktree = TempCwd::new();
    let repo_root = TempCwd::new();
    let granted = format!("{}/.git", repo_root.as_str());

    let conn = AppServerConnection::spawn(&CodexLaunchConfig {
        codex_bin: codex_bin(),
        args: vec!["app-server".to_owned()],
        env: vec![],
    })
    .expect("spawn the real codex app-server");
    conn.initialize(json!({
        "clientInfo": { "name": "delta", "version": env!("CARGO_PKG_VERSION") }
    }))
    .await
    .expect("initialize handshake");

    // Force workspace-write so the effective policy carries writable roots
    // whatever the developer's own default sandbox is.
    let workspace_write = [LaunchOptionSpec {
        name: "sandbox".to_owned(),
        value: Some("workspace-write".to_owned()),
    }];

    // The same launch WITHOUT a worktree, for the replacement note below.
    let baseline = conn
        .start_thread(
            None,
            Some(
                thread_start_params(&worktree.as_str(), &workspace_write, None)
                    .expect("build the baseline params"),
            ),
        )
        .await
        .expect("start a real thread without the grant");

    let started = conn
        .start_thread(
            None,
            Some(
                thread_start_params(
                    &worktree.as_str(),
                    &workspace_write,
                    Some(&repo_root.as_str()),
                )
                .expect("build the granted params"),
            ),
        )
        .await
        .expect("start a real thread with the grant");

    let roots = writable_roots(&started.result);
    eprintln!(
        "writable roots without the grant: {:?}\nwritable roots with the grant:    {roots:?}",
        writable_roots(&baseline.result),
    );
    assert!(
        roots.contains(&granted),
        "the real server must apply the dotted `sandbox_workspace_write.writable_roots` \
         key Delta injects: expected `{granted}` among the thread's effective writable \
         roots, got {roots:?}. Full response: {}",
        started.result
    );
}

/// The writable roots of the sandbox policy a `thread/start` response reports,
/// as plain strings. Empty when the policy is not a `workspaceWrite` one, or
/// carries no roots — either of which fails the assertion above, which is the
/// honest outcome for "the grant did not take".
fn writable_roots(result: &Value) -> Vec<String> {
    result
        .pointer("/sandbox/writableRoots")
        .and_then(Value::as_array)
        .map(|roots| {
            roots
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

// --- Canary 4: schema drift detection --------------------------------------

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
