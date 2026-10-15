//! Starting the server: logging, the loopback listener, the startup errors a
//! user has to act on, and serving the [`router`] on a bound listener.
//!
//! Shared by the `delta-server` binary and the desktop shell (`delta-desktop`) so
//! both start the server the same way. Delta is a local tool and never listens
//! on a public interface, so the only listener offered here is a loopback one.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use delta_usecase::{EraseReport, UpdateRestart};
use tokio::net::TcpListener;
use tracing::level_filters::LevelFilter;
use tracing::Subscriber;
use tracing_log::AsLog;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::EnvFilter;

use crate::config::hook_state::{HookStateError, HookStateFile};
use crate::{router, AppState};

/// How long [`serve`] waits, after it stopped serving, for the session actors
/// to run down and the store to close before it deletes the database anyway.
const STORE_CLOSE_LIMIT: Duration = Duration::from_secs(10);

/// Why [`serve`] returned: the server only stops when something asked it to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStopped {
    /// `POST /api/storage/erase` erased everything Delta created that holds
    /// no work, and the server then deleted its data directory. Carries what
    /// was removed and what was kept. The process should exit `0`.
    Erased(EraseReport),
    /// `POST /api/latest-release/restart` asked to restart into the update
    /// installed over the running app: the release, and the installed app
    /// the installer named for it. The shell should start that app once this
    /// process has exited, then exit `0`. The data directory and the tmux
    /// server are left as they are, so the sessions keep running and the new
    /// process re-adopts them.
    Restart(UpdateRestart),
}

impl ServerStopped {
    /// Log why the server stopped — for an erase, every item removed and kept —
    /// for the shell to call before it exits.
    pub fn log(&self) {
        match self {
            Self::Erased(report) => {
                tracing::info!(
                    sessions = report.removed_sessions.len(),
                    "erased everything Delta created that holds no work; the server has stopped"
                );
                for item in &report.removed {
                    tracing::info!("removed {item}");
                }
                for kept in &report.kept {
                    tracing::info!(session_id = %kept.session_id, "kept {}", kept.kept);
                }
                for kept in &report.kept_leftovers {
                    tracing::info!("kept {kept}");
                }
            }
            Self::Restart(restart) => tracing::info!(
                version = %restart.version,
                app = ?restart.app,
                "the server has stopped to restart into the installed update"
            ),
        }
    }
}

/// Install the global `tracing` subscriber of the `delta-server` binary:
/// stdout only, filtered by `RUST_LOG` when set, `info` otherwise. The desktop
/// app installs its own, which also writes a log file and reads a filter file.
///
/// A filter that traces everything is warned about once installed
/// ([`crate::warn_if_tracing_everything`]).
pub fn init_tracing() {
    let (filter, directives) = match EnvFilter::try_from_default_env() {
        Ok(filter) => (
            filter,
            std::env::var(EnvFilter::DEFAULT_ENV).unwrap_or_default(),
        ),
        Err(_) => (EnvFilter::new("info"), "info".to_owned()),
    };
    install_tracing(server_subscriber(filter, std::io::stdout));
    crate::warn_if_tracing_everything(&directives);
}

/// The `delta-server` binary's subscriber: plain lines to `writer`, filtered
/// by `filter`.
fn server_subscriber<W>(filter: EnvFilter, writer: W) -> impl Subscriber + Send + Sync + 'static
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .finish()
}

/// Install `subscriber` as the process's global `tracing` subscriber, and
/// route the records of the `log` crate into it.
///
/// Delta logs through `tracing`, but some dependencies log through `log`
/// (`rustls-platform-verifier`, for one, warns through it about a system
/// certificate it cannot read). Their records become `tracing` events, so
/// they pass the same filter and reach the same outputs — stdout, and the
/// desktop app's log file — as Delta's own. Each binary calls this once, at
/// startup, with the subscriber it builds.
///
/// # Panics
///
/// When a global subscriber or a `log` logger is already installed.
pub fn install_tracing(subscriber: impl Subscriber + Send + Sync + 'static) {
    tracing::subscriber::set_global_default(subscriber)
        .expect("the global tracing subscriber is installed only once");
    // After the subscriber, so `log` skips at its call site whatever the
    // filter would drop anyway.
    tracing_log::LogTracer::builder()
        .with_max_level(LevelFilter::current().as_log())
        .init()
        .expect("the `log` logger is installed only once");
}

/// Bind `127.0.0.1:<port>`. Port `0` takes a free port; read it back from
/// [`TcpListener::local_addr`].
pub async fn bind_loopback(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await
}

/// Bind the desktop app's listener and set `config.port` to its port.
///
/// The app has no fixed port, so it keeps the one it chose in the hook state
/// file and tries that again first: hook URLs a surviving session holds then
/// still reach the new process. When the recorded port is taken (or none was
/// recorded) it binds a fresh ephemeral port and records that instead; losing a
/// recorded port is logged at `warn` and sets
/// [`Config::hook_endpoint_changed`](delta_bootstrap::Config::hook_endpoint_changed).
///
/// An `explicit` port (`DELTA_PORT`) is bound as-is — failing if it is taken,
/// as before — and is neither compared with nor recorded over the recorded one,
/// so the app's own choice is still there for a launch without it.
pub async fn bind_app_listener(
    config: &mut delta_bootstrap::Config,
    explicit: Option<u16>,
    state: &mut HookStateFile,
) -> anyhow::Result<TcpListener> {
    if let Some(port) = explicit {
        let listener = bind_loopback(port).await?;
        config.port = listener.local_addr()?.port();
        return Ok(listener);
    }
    let recorded = state.port();
    if let Some(port) = recorded {
        match bind_loopback(port).await {
            Ok(listener) => {
                config.port = port;
                return Ok(listener);
            }
            Err(err) => tracing::warn!(
                port,
                "could not bind the port recorded by the previous launch ({err}); \
                 taking a fresh one. Sessions that survived the restart still call \
                 the old port and cannot reach Delta"
            ),
        }
    }
    let listener = bind_loopback(0).await?;
    config.port = listener.local_addr()?.port();
    if recorded.is_some() {
        config.hook_endpoint_changed = true;
    }
    state.record_port(config.port)?;
    Ok(listener)
}

/// The message for a startup failure the user — not Delta — has to act on, or
/// `None` for any other error, which keeps its default propagation.
///
/// A refused overlay is one (the remediation is `make reset`). The migration
/// ladder migrates an out-of-date database forward on its own, so what reaches
/// here is only what it cannot fix: a database written by a newer binary, one
/// that predates the version stamp entirely, or one stamped below the ladder's
/// squashed baseline. The inner store error is returned verbatim — its
/// `Display` already names the remediation.
///
/// A missing host command is another. Its own `Display` names the command, so
/// that line is returned as-is: installing tmux is the user's business and Delta
/// has no advice to give about how.
///
/// A hook state file that cannot be read or written is a third: its `Display`
/// names the file, and the remedy (the directory's permissions, or deleting
/// the file) is on the user's machine.
pub fn user_facing_startup_error(err: &anyhow::Error) -> Option<String> {
    if let Some(state_err) = err.downcast_ref::<HookStateError>() {
        return Some(state_err.to_string());
    }
    match err.downcast_ref::<delta_bootstrap::Error>()? {
        delta_bootstrap::Error::Store(
            store_err @ (delta_bootstrap::StoreError::SchemaMismatch { .. }
            | delta_bootstrap::StoreError::UnstampedOverlay
            | delta_bootstrap::StoreError::PreBaselineOverlay { .. }),
        ) => Some(store_err.to_string()),
        missing @ delta_bootstrap::Error::MissingCommand { .. } => Some(missing.to_string()),
        _ => None,
    }
}

/// Rewrite the session settings file, start the background loops `state`
/// needs, and serve the [`router`] on `listener` until something asks the
/// server to stop, returning why.
///
/// The listener's port must be the one the configuration `state` was built from
/// names, because the hook URLs rendered into each session's settings carry it.
///
/// Stopping is graceful: the request that asked for it still gets its
/// response. Then the background loops are aborted and the state dropped, and
/// for an erase, once the store has closed — the session actors hold it until
/// they have run down — the data directory is deleted (see
/// `StorageInventory::delete_data_dir`). A restart leaves everything as it
/// is.
pub async fn serve(state: AppState, listener: TcpListener) -> anyhow::Result<ServerStopped> {
    // Rewrite the session settings file now, so a restart leaves it matching
    // this run's hook URLs instead of a stale copy (see
    // `refresh_session_settings`). Not fatal: every spawn and resume writes it
    // again and reports its own failure.
    if let Err(err) = state.interactor().refresh_session_settings().await {
        tracing::warn!("could not rewrite the session settings file at startup: {err}");
    }

    // Continuously tail the transcript so assistant replies that Claude Code
    // flushes after the `Stop` hook still reach the browser within ~0.5s.
    let tail = state.spawn_transcript_tail();

    // Drain the interactor's async event seam into the broadcast, so a producer
    // that emits after its driving call returned still reaches browsers. This is
    // the only consumer of that seam, so without it those events go nowhere; for
    // who emits on it, see `Interactor::emit_async_event`.
    let drain = state.spawn_async_event_drain();

    // Check for a newer published release in the background, off the startup
    // path; `None` when `DELTA_RELEASE_FEED_URL` turned the check off.
    let release_check = state.spawn_release_check();

    let app = router(state.clone());
    let addr = listener.local_addr()?;
    tracing::info!(%addr, "delta-server listening (loopback only)");

    axum::serve(listener, app)
        .with_graceful_shutdown(state.stopping())
        .await?;
    let reason = state
        .take_stop_reason()
        .ok_or_else(|| anyhow::anyhow!("the server stopped serving without being asked to"))?;
    tracing::info!("delta-server stopped serving");

    for task in std::iter::once(tail).chain(drain).chain(release_check) {
        task.abort();
        // Aborted on purpose; awaiting is only to know the task has dropped
        // what it held (the interactor, for the tail).
        let _ = task.await;
    }
    match &reason {
        ServerStopped::Erased(_) => delete_data_dir_once_closed(state).await,
        ServerStopped::Restart(_) => {}
    }
    Ok(reason)
}

/// Drop the last state this function holds, wait for the store to close (why:
/// `StorageInventory::delete_data_dir`), and delete the data directory.
///
/// A store still open after [`STORE_CLOSE_LIMIT`] is logged, and the directory
/// deleted all the same — the user asked for it.
async fn delete_data_dir_once_closed(state: AppState) {
    let storage = state.storage().clone();
    let release = state.interactor().core_release();
    drop(state);
    if !release.released_within(STORE_CLOSE_LIMIT).await {
        tracing::warn!(
            limit_secs = STORE_CLOSE_LIMIT.as_secs(),
            "the store was still open after the server stopped; deleting the data directory anyway"
        );
    }
    storage.delete_data_dir();
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::hook_state::HookStateFile;

    /// A configuration whose hook state file lives in `dir`.
    fn config_in(dir: &tempfile::TempDir) -> delta_bootstrap::Config {
        let data_dir = dir.path().to_string_lossy().into_owned();
        crate::config::config_from_vars(|name| {
            (name == "DELTA_DATA_DIR").then(|| data_dir.clone().into())
        })
    }

    fn state_in(config: &delta_bootstrap::Config) -> HookStateFile {
        HookStateFile::open(config.data_layout().hook_state()).unwrap()
    }

    /// A port that is free right now: bound and released.
    async fn free_port() -> u16 {
        bind_loopback(0).await.unwrap().local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn a_first_launch_takes_a_fresh_port_and_records_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = config_in(&dir);
        let mut state = state_in(&config);

        let listener = bind_app_listener(&mut config, None, &mut state)
            .await
            .unwrap();

        let port = listener.local_addr().unwrap().port();
        assert_eq!(config.port, port);
        assert!(
            !config.hook_endpoint_changed,
            "no recorded port, so nothing to lose"
        );
        assert_eq!(state_in(&config).port(), Some(port));
    }

    #[tokio::test]
    async fn the_recorded_port_is_bound_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = config_in(&dir);
        let recorded = free_port().await;
        state_in(&config).record_port(recorded).unwrap();
        let mut state = state_in(&config);

        let listener = bind_app_listener(&mut config, None, &mut state)
            .await
            .unwrap();

        assert_eq!(listener.local_addr().unwrap().port(), recorded);
        assert_eq!(config.port, recorded);
        assert!(!config.hook_endpoint_changed);
    }

    #[tokio::test]
    async fn a_taken_recorded_port_falls_back_to_a_fresh_one_and_reports_the_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = config_in(&dir);
        let occupant = bind_loopback(0).await.unwrap();
        let taken = occupant.local_addr().unwrap().port();
        state_in(&config).record_port(taken).unwrap();
        let mut state = state_in(&config);

        let listener = bind_app_listener(&mut config, None, &mut state)
            .await
            .unwrap();

        let port = listener.local_addr().unwrap().port();
        assert_ne!(port, taken);
        assert_eq!(config.port, port);
        assert!(config.hook_endpoint_changed);
        assert_eq!(
            state_in(&config).port(),
            Some(port),
            "the fresh port is the one the next launch tries"
        );
        drop(occupant);
    }

    #[tokio::test]
    async fn an_explicit_port_is_bound_and_not_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = config_in(&dir);
        let recorded = free_port().await;
        state_in(&config).record_port(recorded).unwrap();
        let mut state = state_in(&config);
        let explicit = free_port().await;

        let listener = bind_app_listener(&mut config, Some(explicit), &mut state)
            .await
            .unwrap();

        assert_eq!(listener.local_addr().unwrap().port(), explicit);
        assert_eq!(config.port, explicit);
        assert!(!config.hook_endpoint_changed);
        assert_eq!(
            state_in(&config).port(),
            Some(recorded),
            "the app's own choice is kept for a launch without DELTA_PORT"
        );
    }

    #[tokio::test]
    async fn a_taken_explicit_port_fails_the_start() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = config_in(&dir);
        let occupant = bind_loopback(0).await.unwrap();
        let taken = occupant.local_addr().unwrap().port();
        let mut state = state_in(&config);

        assert!(bind_app_listener(&mut config, Some(taken), &mut state)
            .await
            .is_err());
        assert_eq!(state_in(&config).port(), None);
        drop(occupant);
    }

    #[test]
    fn a_hook_state_failure_is_user_facing() {
        let err = anyhow::Error::from(HookStateError::Write {
            path: "/data/delta-hook-state.json".into(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        });
        let message = user_facing_startup_error(&err).expect("user-facing");
        assert!(message.contains("/data/delta-hook-state.json"), "{message}");
    }

    /// Where the subscriber under test writes.
    #[derive(Clone, Default)]
    struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Captured;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn a_log_record_reaches_the_installed_subscriber_through_its_filter() {
        // The only test in this binary that installs the global subscriber.
        // Its filter admits only this test's target, so the other tests'
        // events stay out of it.
        let captured = Captured::default();
        install_tracing(server_subscriber(
            EnvFilter::new("delta_log_bridge=info"),
            captured.clone(),
        ));

        log::warn!(target: "delta_log_bridge", "could not read a system certificate");
        log::debug!(target: "delta_log_bridge", "below the filter");

        let text = captured.text();
        assert!(text.contains("WARN"), "{text}");
        assert!(text.contains("delta_log_bridge"), "{text}");
        assert_eq!(
            text.matches("could not read a system certificate").count(),
            1,
            "{text}"
        );
        assert!(!text.contains("below the filter"), "{text}");
    }
}
