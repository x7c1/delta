//! HTTP-level tests against the assembled router, split by endpoint group.
//!
//! A fixture stays in its group's own file and moves here once a second group
//! needs it — as the gh stub pair did: `clone_roots` calls
//! `test_state_with_gh_stub`, and `pull_requests` calls its
//! `test_state_with_unavailable_gh` wrapper.

mod auth;
mod clone_roots;
mod hooks;
mod launch_options;
mod origin_guard;
mod permissions;
mod prompt_templates;
mod providers;
mod pull_requests;
mod session_prune;
mod sessions;
mod static_web;
mod status_line;
mod storage;
mod storage_erase;
mod storage_snapshots;
mod storage_worktrees;
mod workdir;

use super::{router, AppState};
use delta_bootstrap::Config;
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// The bearer token every state built in these tests holds, so a request the
/// tests drive through the router can present a valid token and pass the auth
/// guard. The `origin_guard` group's no-token / wrong-token cases use it as the
/// baseline they deviate from.
pub(super) const TEST_AUTH_TOKEN: &str = "delta-test-auth-token";

/// The hook secret every state built in these tests holds, so a request
/// the tests drive at `/hooks/*` can present a valid secret and pass the hook
/// auth guard. The hook group's negative case uses it as the baseline it
/// deviates from.
pub(super) const TEST_HOOK_SECRET: &str = "delta-test-hook-secret";

/// The transcript root every state built in these tests confines hook-reported
/// transcript paths to. The tests point their `transcript_path`s under `/tmp`,
/// so this is `/tmp` (canonicalized to `/private/tmp` on macOS by the guard).
pub(super) const TEST_TRANSCRIPT_ROOT: &str = "/tmp";

/// The `Authorization` header value carrying [`TEST_AUTH_TOKEN`], so every
/// router-driving request can attach a valid bearer token in one call.
pub(super) fn bearer() -> String {
    format!("Bearer {TEST_AUTH_TOKEN}")
}

/// Drive one request with an optional JSON body through the router and read
/// back its status and body: JSON, `null` when empty, or a string when it is
/// not JSON.
pub(super) async fn request_json(
    state: &AppState,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (axum::http::StatusCode, serde_json::Value) {
    use tower::ServiceExt;

    let mut request = axum::http::Request::builder()
        .header("host", "127.0.0.1")
        .header("authorization", bearer())
        .method(method)
        .uri(uri);
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let body = body.map_or_else(axum::body::Body::empty, |body| {
        axum::body::Body::from(body.to_owned())
    });
    let response = router(state.clone())
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    // An extractor's rejection (a malformed query or body) is plain text, not
    // the JSON error body; it is returned as a string.
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
        })
    };
    (status, value)
}

/// The `?hs=<secret>` query string carrying [`TEST_HOOK_SECRET`], appended to a
/// `/hooks/*` path so a router-driving hook request clears the hook auth guard.
pub(super) fn hook_query() -> String {
    format!("?hs={TEST_HOOK_SECRET}")
}

thread_local! {
    /// The data directories of the states built on this thread.
    ///
    /// A state's database lives in its data directory, which has to outlive the
    /// state, but the states are handed out bare. Each test runs on a thread of
    /// its own (libtest spawns one per test, and a `#[tokio::test]` runtime runs
    /// on it), so the directories are removed when the test that built them
    /// ends.
    static DATA_DIRS: RefCell<Vec<tempfile::TempDir>> = const { RefCell::new(Vec::new()) };
}

/// The configuration every state built in these tests starts from, in a fresh
/// data directory of its own, so no two tests share a database.
pub(super) fn test_config() -> Config {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().to_string_lossy().into_owned();
    DATA_DIRS.with_borrow_mut(|dirs| dirs.push(dir));
    Config {
        identifier: "delta-test".into(),
        data_dir,
        worktree_base: "/tmp/delta-test-worktrees".into(),
        tmux_socket: "delta-test".into(),
        auth_token: TEST_AUTH_TOKEN.into(),
        hook_secret: TEST_HOOK_SECRET.into(),
        transcript_root: TEST_TRANSCRIPT_ROOT.into(),
        port: 7878,
        hook_endpoint_changed: false,
        launch: delta_usecase::LaunchConfig::default(),
    }
}

async fn test_state() -> AppState {
    AppState::build(&Config {
        launch: delta_usecase::LaunchConfig {
            // The permission-request hook test exercises the no-decision
            // passthrough, which waits out this deadline; keep it short.
            permission_decision_deadline: std::time::Duration::from_millis(50),
            ..delta_usecase::LaunchConfig::default()
        },
        ..test_config()
    })
    .await
    .unwrap()
}

/// Build a `test_state()` whose gh CLI is stubbed to report
/// "unavailable", so the PR-route smoke tests are independent of
/// whether `gh` happens to be installed on the test host.
async fn test_state_with_unavailable_gh() -> AppState {
    test_state_with_gh_stub().await.0
}

/// Like [`test_state_with_unavailable_gh`], but also hands back the counter
/// of `clone_repo` invocations the stub has seen.
///
/// The clone route's refusals are meant to start no job at all, and "no job"
/// is only observable as "gh was never invoked" — this counter is that
/// observation.
async fn test_state_with_gh_stub() -> (AppState, Arc<AtomicUsize>) {
    // Start from the shared `test_config()`, then override the
    // wired Interactor's gh driver with a deterministic stub.
    struct UnavailableGh {
        clone_calls: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl delta_usecase::GhCli for UnavailableGh {
        async fn is_authenticated(&self) -> bool {
            false
        }
        async fn search_prs(
            &self,
            _lens: delta_usecase::PullRequestLens,
        ) -> delta_usecase::Result<Vec<delta_usecase::PullRequest>> {
            Ok(Vec::new())
        }
        async fn clone_repo(
            &self,
            _owner: &str,
            _name: &str,
            _destination: &str,
        ) -> delta_usecase::Result<()> {
            self.clone_calls.fetch_add(1, Ordering::SeqCst);
            // The route tests only care that a job did (or did not) start;
            // what the clone then does is the use case's own tests' subject.
            Err(delta_usecase::Error::Gh("stubbed clone".into()))
        }
    }
    let clone_calls = Arc::new(AtomicUsize::new(0));
    let gh = Arc::new(UnavailableGh {
        clone_calls: Arc::clone(&clone_calls),
    });
    let config = test_config();
    let interactor = delta_bootstrap::build(&config, delta_usecase::NullCommsLog::arc())
        .await
        .unwrap()
        .with_gh_cli(gh as Arc<dyn delta_usecase::GhCli>);
    (
        AppState::from_interactor(
            interactor,
            &config.tmux_socket,
            &config.auth_token,
            &config.hook_secret,
            crate::StorageInventory::from_config(&config),
        ),
        clone_calls,
    )
}
