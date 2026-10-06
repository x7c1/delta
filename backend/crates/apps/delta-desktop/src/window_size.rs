//! The window's size: remembered between launches, and picked from the screen
//! the first time.
//!
//! `tauri-plugin-window-state`, registered in `main`, remembers the size and
//! the maximized state. It loads its state file
//! (`<app config dir>/.window-state.json`) as the app is built, restores the
//! window once it is created, and writes the file as the app exits. The
//! position is left out on purpose: Wayland does not let an app place its
//! window, so restoring a position would work on macOS and not on Linux, and
//! the compositor (or [`fit_window_size`]'s centring) places it
//! instead.
//!
//! The first time, when the state file holds no size for the window, the size
//! comes from the screen the window opens on ([`initial_size`]). Not
//! maximized: on a large display a maximized window is too wide for a
//! chat-and-terminal layout, and a user who wants it maximizes it once and the
//! state is remembered.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, LogicalSize, Manager, Monitor, Runtime, WebviewWindow, WindowEvent};
use tauri_plugin_window_state::AppHandleExt;

/// The smallest window the SPA's layout works at, in logical pixels (CSS
/// pixels in the webview).
///
/// Width: the workspace puts the navigator, the conversation and the terminal
/// side by side only from a 1024px-wide viewport (`useMediaQuery('(min-width:
/// 1024px)')` in `WorkspaceScreen.tsx`); below it the terminal turns into an
/// overlay drawn over the conversation. At 1024 the navigator's 288px
/// (`w-72`) and the terminal's default 384px leave the conversation 352px.
///
/// Height: 640 keeps the conversation's header, the composer and its options
/// row in view with about a dozen lines of conversation and terminal above
/// them; it is also what 80 % of a 1280×800 screen's work area comes to, so
/// the smallest common laptop screen opens at the minimum.
pub const MIN_SIZE: LogicalSize<f64> = LogicalSize {
    width: 1024.0,
    height: 640.0,
};

/// The size the window is built at, and keeps when no monitor can be read on
/// the first launch.
pub const FALLBACK_SIZE: LogicalSize<f64> = LogicalSize {
    width: 1280.0,
    height: 800.0,
};

// The fallback is a size the window may have.
const _: () =
    assert!(FALLBACK_SIZE.width >= MIN_SIZE.width && FALLBACK_SIZE.height >= MIN_SIZE.height);

/// The share of the monitor's work area the first window takes, per side.
const WORK_AREA_SHARE: f64 = 0.8;

/// The first window's size for a monitor whose work area (the screen less the
/// menu bar, the Dock or the panels) is `work_area` logical pixels: 80 % of it
/// per side, never below [`MIN_SIZE`]. Without a monitor, [`FALLBACK_SIZE`].
pub fn initial_size(work_area: Option<LogicalSize<f64>>) -> LogicalSize<f64> {
    match work_area {
        Some(area) => LogicalSize {
            width: (area.width * WORK_AREA_SHARE).round().max(MIN_SIZE.width),
            height: (area.height * WORK_AREA_SHARE).round().max(MIN_SIZE.height),
        },
        None => FALLBACK_SIZE,
    }
}

/// On the first launch — when the plugin has no size to restore for the window
/// labelled `label` — size `window` from the monitor it opened on and centre
/// it. Any other launch keeps the plugin's restored size, raised to
/// [`MIN_SIZE`] if it is below it.
///
/// Call it right after the window is built. The plugin's restore has not run
/// yet then: Tauri hands the new window to plugins later, on the main thread.
/// On a first launch that does not matter, as the plugin has nothing to
/// restore and saves the window's live size as the app exits; otherwise the
/// size is checked against the minimum each time the window is resized, which
/// includes the restore's own resize.
pub fn fit_window_size<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>, label: &str) {
    if has_remembered_size(app, label) {
        let restored = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::Resized(size) = event {
                if size.width > 0 && size.height > 0 {
                    raise_to_minimum(&restored);
                }
            }
        });
        return;
    }
    let size = initial_size(monitor_of(window).map(|monitor| logical_work_area(&monitor)));
    tracing::info!(
        width = size.width,
        height = size.height,
        "no remembered window size; sizing the window from the screen"
    );
    // macOS applies the new size asynchronously, so a `center()` right after
    // `set_size` centres the old size and the larger window then runs off the
    // right of a wide screen. Centre again once, when the resize lands.
    let resized = window.clone();
    let centred = AtomicBool::new(false);
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Resized(_)) && !centred.swap(true, Ordering::Relaxed) {
            centre(&resized);
        }
    });
    if let Err(err) = window.set_size(size) {
        tracing::warn!("could not size the window from the screen: {err}");
    }
    centre(window);
}

fn centre<R: Runtime>(window: &WebviewWindow<R>) {
    if let Err(err) = window.center() {
        tracing::warn!("could not centre the window: {err}");
    }
}

/// Raise a window size below [`MIN_SIZE`] to it. The plugin stores and
/// restores physical pixels, so a size saved on a screen of scale factor 1
/// comes back at half its logical size on a scale-2 one; and `min_inner_size`
/// only stops the user from resizing below the minimum, not `set_size`.
fn raise_to_minimum<R: Runtime>(window: &WebviewWindow<R>) {
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let size = match (window.inner_size(), window.scale_factor()) {
        (Ok(size), Ok(scale)) => size.to_logical::<f64>(scale),
        (Err(err), _) | (_, Err(err)) => {
            tracing::warn!("could not read the window size: {err}");
            return;
        }
    };
    let Some(raised) = at_least_minimum(size) else {
        return;
    };
    tracing::info!(
        width = raised.width,
        height = raised.height,
        "the window size is below the minimum; raising it"
    );
    if let Err(err) = window.set_size(raised) {
        tracing::warn!("could not raise the window to the minimum size: {err}");
    }
}

/// `size` raised to [`MIN_SIZE`] per side, or `None` when it is not below it.
fn at_least_minimum(size: LogicalSize<f64>) -> Option<LogicalSize<f64>> {
    let raised = LogicalSize {
        width: size.width.max(MIN_SIZE.width),
        height: size.height.max(MIN_SIZE.height),
    };
    (raised != size).then_some(raised)
}

/// Whether the plugin's state file holds a size for the window `label`. A file
/// that cannot be read or parsed restores nothing either, so it counts as none.
fn has_remembered_size<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    let path = match app.path().app_config_dir() {
        Ok(dir) => dir.join(app.filename()),
        Err(err) => {
            tracing::warn!("could not resolve the window state file: {err}");
            return false;
        }
    };
    match std::fs::read_to_string(&path) {
        Ok(contents) => {
            let remembered = remembers_size(&contents, label);
            if !remembered {
                tracing::warn!(
                    path = %path.display(),
                    "the window state file holds no size for the window; treating this as a first launch"
                );
            }
            remembered
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => false,
        Err(err) => {
            tracing::warn!(path = %path.display(), "could not read the window state file: {err}");
            false
        }
    }
}

/// Whether the state file's `contents` — a JSON object of window states keyed
/// by label — give the window `label` a non-zero size, which is what the plugin
/// needs to restore it (it skips an all-default entry).
fn remembers_size(contents: &str, label: &str) -> bool {
    let Ok(states) = serde_json::from_str::<serde_json::Value>(contents) else {
        return false;
    };
    let dimension = |key: &str| {
        states
            .get(label)
            .and_then(|state| state.get(key))
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|value| value > 0)
    };
    dimension("width") && dimension("height")
}

/// The monitor the window is on, or the primary one; `None` when neither can
/// be read.
fn monitor_of<R: Runtime>(window: &WebviewWindow<R>) -> Option<Monitor> {
    let current = window.current_monitor().unwrap_or_else(|err| {
        tracing::warn!("could not read the window's monitor: {err}");
        None
    });
    current.or_else(|| {
        window.primary_monitor().unwrap_or_else(|err| {
            tracing::warn!("could not read the primary monitor: {err}");
            None
        })
    })
}

/// The monitor's work area in logical pixels.
fn logical_work_area(monitor: &Monitor) -> LogicalSize<f64> {
    monitor.work_area().size.to_logical(monitor.scale_factor())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: f64, height: f64) -> LogicalSize<f64> {
        LogicalSize { width, height }
    }

    #[test]
    fn a_full_hd_work_area_gives_80_percent_of_it() {
        assert_eq!(
            initial_size(Some(size(1920.0, 1080.0))),
            size(1536.0, 864.0)
        );
    }

    #[test]
    fn the_size_is_rounded_to_whole_logical_pixels() {
        assert_eq!(initial_size(Some(size(1512.0, 945.0))), size(1210.0, 756.0));
    }

    #[test]
    fn a_tiny_work_area_is_clamped_to_the_minimum() {
        assert_eq!(initial_size(Some(size(800.0, 600.0))), MIN_SIZE);
    }

    #[test]
    fn each_side_is_clamped_on_its_own() {
        assert_eq!(initial_size(Some(size(2560.0, 700.0))), size(2048.0, 640.0));
    }

    #[test]
    fn without_a_monitor_the_window_keeps_the_fallback_size() {
        assert_eq!(initial_size(None), FALLBACK_SIZE);
    }

    #[test]
    fn a_restored_size_below_the_minimum_is_raised_per_side() {
        assert_eq!(at_least_minimum(size(500.0, 300.0)), Some(MIN_SIZE));
        assert_eq!(
            at_least_minimum(size(1600.0, 300.0)),
            Some(size(1600.0, 640.0))
        );
    }

    #[test]
    fn a_restored_size_at_or_above_the_minimum_is_kept() {
        assert_eq!(at_least_minimum(MIN_SIZE), None);
        assert_eq!(at_least_minimum(size(2400.0, 1500.0)), None);
    }

    #[test]
    fn a_saved_size_for_the_window_is_remembered() {
        let contents = r#"{"main":{"width":2400,"height":1500,"x":0,"y":0,"prev_x":0,"prev_y":0,"maximized":false,"visible":true,"decorated":true,"fullscreen":false}}"#;
        assert!(remembers_size(contents, "main"));
    }

    #[test]
    fn a_state_file_without_the_window_remembers_nothing() {
        assert!(!remembers_size(
            r#"{"other":{"width":800,"height":600}}"#,
            "main"
        ));
        assert!(!remembers_size("{}", "main"));
    }

    #[test]
    fn a_zero_size_remembers_nothing() {
        assert!(!remembers_size(
            r#"{"main":{"width":0,"height":0}}"#,
            "main"
        ));
    }

    #[test]
    fn a_corrupt_state_file_remembers_nothing() {
        assert!(!remembers_size("{\"main\":", "main"));
        assert!(!remembers_size("", "main"));
    }
}
