//! HTTP-level tests against the assembled router, split by endpoint group.
//!
//! A fixture stays in its group's own file and moves here once a second group
//! needs it — as the gh stub pair did: `clone_roots` calls
//! `test_state_with_gh_stub`, and `pull_requests` calls its
//! `test_state_with_unavailable_gh` wrapper.

mod clone_roots;
mod hooks;
mod launch_options;
mod origin_guard;
mod permissions;
mod prompt_templates;
mod providers;
mod pull_requests;
mod sessions;
mod status_line;
mod workdir;

use super::{router, AppState};
use delta_bootstrap::Config;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

async fn test_state() -> AppState {
    AppState::build(&Config {
        database_path: ":memory:".into(),
        session_workdir_base: "/tmp/delta-test-session".into(),
        worktree_base: "/tmp/delta-test-worktrees".into(),
        tmux_socket: "delta-test".into(),
        port: 7878,
        launch: delta_usecase::LaunchConfig {
            // The permission-request hook test exercises the no-decision
            // passthrough, which waits out this deadline; keep it short.
            permission_decision_deadline: std::time::Duration::from_millis(50),
            ..delta_usecase::LaunchConfig::default()
        },
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
    // Mirror `test_state()`'s config exactly, then override the
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
    let config = delta_bootstrap::Config {
        database_path: ":memory:".into(),
        session_workdir_base: "/tmp/delta-test-session".into(),
        worktree_base: "/tmp/delta-test-worktrees".into(),
        tmux_socket: "delta-test".into(),
        port: 7878,
        launch: delta_usecase::LaunchConfig::default(),
    };
    let interactor = delta_bootstrap::build(&config, delta_usecase::NullCommsLog::arc())
        .await
        .unwrap()
        .with_gh_cli(gh as Arc<dyn delta_usecase::GhCli>);
    (
        AppState::from_interactor(interactor, &config.tmux_socket),
        clone_calls,
    )
}
