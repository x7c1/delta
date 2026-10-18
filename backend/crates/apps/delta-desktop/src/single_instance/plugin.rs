use std::path::PathBuf;
use std::sync::Arc;

use tauri::plugin::{self, TauriPlugin};
use tauri::{AppHandle, Manager, RunEvent, Runtime};

use super::claim::{claim, Claim};
use super::ensure_private_dir::ensure_private_dir;
use super::instance_lock::InstanceLock;
use super::socket_path::socket_path;
use super::user_temp_dir::user_temp_dir;
use crate::logging::LogGuard;

/// The single-instance lock as a Tauri plugin, to be registered before any
/// other: its `setup` runs first, so a second launch tells the running copy and
/// exits 0 before any other plugin or the app's own `setup` runs, as
/// `tauri-plugin-single-instance` does. The running copy calls
/// `on_second_launch` for each later launch, off the main thread, and removes
/// its socket on [`RunEvent::Exit`].
///
/// When the lock cannot be set up, the reason is logged and the app launches
/// without it.
pub fn init<R: Runtime>(
    on_second_launch: impl Fn(&AppHandle<R>) + Send + Sync + 'static,
) -> TauriPlugin<R> {
    plugin::Builder::new("delta-single-instance")
        .setup(move |app, _api| {
            let claimed = lock_path(&app.config().identifier).and_then(|path| {
                let handle = app.clone();
                claim(&path, move || on_second_launch(&handle))
            });
            match claimed {
                Ok(Claim::AlreadyRunning) => {
                    tracing::info!("Delta is already running; brought it forward and exiting");
                    if let Some(log_guard) = app.try_state::<Arc<LogGuard>>() {
                        log_guard.flush();
                    }
                    std::process::exit(0);
                }
                Ok(Claim::Acquired(lock)) => {
                    app.manage(lock);
                }
                Err(err) => tracing::warn!(
                    "could not set up the single-instance lock, launching without it: {err:#}"
                ),
            }
            Ok(())
        })
        .on_event(|app, event| {
            if let RunEvent::Exit = event {
                if let Some(lock) = app.try_state::<InstanceLock>() {
                    lock.release();
                }
            }
        })
        .build()
}

/// The lock's socket for the app `identifier`, in the user's own temporary
/// directory.
///
/// Builds before this lock kept a socket of the same name in `/tmp`, where
/// this one does not look: a copy of such a build still running is not
/// detected. Nothing needs to handle that: an update relaunches only once the
/// old process has exited (`relaunch.rs`), so both run only when the user
/// starts an old and a new build at once.
fn lock_path(identifier: &str) -> anyhow::Result<PathBuf> {
    let dir = user_temp_dir()?;
    ensure_private_dir(&dir)?;
    socket_path(&dir, identifier)
}
