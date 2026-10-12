use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, LogicalSize, Manager, Runtime, WebviewWindow, WindowEvent};

use super::{
    fit_to_screen, follow_resizes, initial_size, read_remembered, state_file, RememberedSize,
    WindowMode, WorkArea,
};

/// Restore the remembered size on `window`, fitted to the monitor it opens on
/// ([`fit_to_screen`](fn@fit_to_screen));
/// the first time, size it from that monitor and centre it. Then follow its
/// resizes into [`RememberedSize`].
///
/// Call it right after the window is built, before the window-state plugin
/// restores the maximized state (Tauri hands the new window to plugins later,
/// on the main thread).
pub fn fit_window_size<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) {
    let work_area = WorkArea::of_window(window);
    let remembered = state_file(app).and_then(|path| read_remembered(&path));
    let size = match remembered {
        Some(saved) => {
            let size = fit_to_screen(saved, work_area);
            tracing::info!(
                saved_width = saved.width,
                saved_height = saved.height,
                saved_screen_width = saved.screen.map(|screen| screen.width),
                saved_screen_height = saved.screen.map(|screen| screen.height),
                width = size.width,
                height = size.height,
                "restoring the remembered window size"
            );
            if let Err(err) = window.set_size(size) {
                tracing::warn!("could not restore the window size: {err}");
            }
            size
        }
        None => {
            let size = initial_size(work_area.map(WorkArea::logical));
            tracing::info!(
                width = size.width,
                height = size.height,
                "no remembered window size; sizing the window from the screen"
            );
            size_from_screen(window, size);
            size
        }
    };
    app.state::<RememberedSize>().observe(
        size,
        WindowMode::Normal,
        work_area.map(WorkArea::logical),
    );
    follow_resizes(app, window);
}

/// Size a first-launch window to `size` and centre it.
fn size_from_screen<R: Runtime>(window: &WebviewWindow<R>, size: LogicalSize<f64>) {
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
