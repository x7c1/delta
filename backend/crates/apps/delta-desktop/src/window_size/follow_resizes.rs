use tauri::{AppHandle, LogicalSize, Manager, Runtime, WebviewWindow, WindowEvent};

use super::{RememberedSize, WindowMode, WorkArea};

/// Record the window's content size in [`RememberedSize`] each time it is
/// resized, with the work area of the monitor it is on then.
pub fn follow_resizes<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) {
    let app = app.clone();
    let resized = window.clone();
    window.on_window_event(move |event| {
        if !matches!(event, WindowEvent::Resized(_)) {
            return;
        }
        match content_size_and_mode(&resized) {
            Ok((size, mode)) => app.state::<RememberedSize>().observe(
                size,
                mode,
                WorkArea::of_window(&resized).map(WorkArea::logical),
            ),
            Err(err) => tracing::warn!("could not read the window size: {err}"),
        }
    });
}

/// The window's content size in logical pixels, read from GTK the way
/// `set_size` applies it (`gtk_window_get_size` against `gtk_window_resize`),
/// and its mode, from the GDK window's state, which GDK updates before it
/// delivers the configure event that ends in `Resized` (tao's own maximized
/// flag follows only on the later window-state event).
#[cfg(target_os = "linux")]
fn content_size_and_mode<R: Runtime>(
    window: &WebviewWindow<R>,
) -> tauri::Result<(LogicalSize<f64>, WindowMode)> {
    use gtk::gdk::WindowState;
    use gtk::prelude::{GtkWindowExt, WidgetExt};

    let gtk_window = window.gtk_window()?;
    let (width, height) = gtk_window.size();
    let state = gtk_window
        .window()
        .map_or(WindowState::empty(), |gdk_window| gdk_window.state());
    let mode = if state.contains(WindowState::ICONIFIED) {
        WindowMode::Minimized
    } else if state.contains(WindowState::FULLSCREEN) {
        WindowMode::Fullscreen
    } else if state.contains(WindowState::MAXIMIZED) {
        WindowMode::Maximized
    } else {
        WindowMode::Normal
    };
    Ok((LogicalSize::new(width.into(), height.into()), mode))
}

/// The window's content size in logical pixels, and its mode.
#[cfg(not(target_os = "linux"))]
fn content_size_and_mode<R: Runtime>(
    window: &WebviewWindow<R>,
) -> tauri::Result<(LogicalSize<f64>, WindowMode)> {
    let size = window.inner_size()?.to_logical(window.scale_factor()?);
    let mode = if window.is_minimized()? {
        WindowMode::Minimized
    } else if window.is_fullscreen()? {
        WindowMode::Fullscreen
    } else if window.is_maximized()? {
        WindowMode::Maximized
    } else {
        WindowMode::Normal
    };
    Ok((size, mode))
}
