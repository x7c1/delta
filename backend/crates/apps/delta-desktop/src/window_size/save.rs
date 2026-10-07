use tauri::{AppHandle, Manager, Runtime};

use super::{state_file, write_remembered, RememberedSize};

/// Write the window's last normal size to the state file. Called once, as the
/// app exits; see the module doc for the exits that reach it.
pub fn save<R: Runtime>(app: &AppHandle<R>) {
    let Some(saved) = app.state::<RememberedSize>().last_normal() else {
        tracing::info!("the window never had a normal size; not saving one");
        return;
    };
    let Some(path) = state_file(app) else {
        return;
    };
    match write_remembered(&path, saved) {
        Ok(()) => tracing::info!(
            path = %path.display(),
            width = saved.width,
            height = saved.height,
            screen_width = saved.screen.map(|screen| screen.width),
            screen_height = saved.screen.map(|screen| screen.height),
            "saved the window size"
        ),
        Err(err) => {
            tracing::warn!(path = %path.display(), "could not save the window size: {err}")
        }
    }
}
