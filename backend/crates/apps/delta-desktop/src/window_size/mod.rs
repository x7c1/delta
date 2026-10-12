//! The window's size: remembered between launches, and picked from the screen
//! the first time.
//!
//! Delta remembers the window's **content** size itself, in logical pixels,
//! with the logical size of the screen (the monitor's work area) it was made
//! on, in `<app config dir>/window-size.json` ([`STATE_FILE_NAME`]).
//! `tauri-plugin-window-state`, registered in `main`, is kept for the
//! maximized state only.
//!
//! Why not the plugin's size: it saves the size tao reports and restores it
//! with `set_size`, and on Linux the two do not mean the same thing. tao's
//! reported size (the `Resized` event and `inner_size()`) comes from GDK's
//! configure event, which is the whole toplevel including the client-side
//! decorations (header bar and shadow), while `set_size` goes through
//! `gtk_window_resize`, which sets the content size. Each save added the
//! decoration and each restore kept it, so the window grew on every launch. On
//! Linux the size is therefore read from GTK (`gtk_window_get_size`), which has
//! the same units and meaning as `gtk_window_resize`; on macOS tao's
//! `inner_size()` already is the content size. Both are kept in logical pixels
//! and restored with a logical `set_size`, so one mechanism serves both
//! platforms, and a size saved on a monitor of another scale keeps its meaning.
//! The sizes the plugin saved (too large on Linux) are not read: until Delta
//! has saved a size of its own, the window is sized from the screen as on a
//! first launch.
//!
//! Lifecycle:
//!
//! - [`fit_window_size`](fn@fit_window_size), right after the window is
//!   built, restores the remembered size fitted to the monitor the window
//!   opens on ([`fit_to_screen`](fn@fit_to_screen)): exactly on a screen at
//!   least as large as the one it was made on, so a window stretched across
//!   the screen comes back the same, and cut to the screen (on Linux with a
//!   margin for the decoration and the desktop's bars) on a smaller one or
//!   when that screen is not known. Or, the first time, when nothing is
//!   remembered, it sizes the window from that monitor
//!   ([`initial_size`](fn@initial_size)) and centres it. Not maximized: on a
//!   large display a maximized window is too wide for a chat-and-terminal
//!   layout, and a user who wants it maximizes it once and the plugin
//!   remembers that. The plugin maximizes the window later, when Tauri hands it
//!   the window, so unmaximizing goes back to the restored size.
//! - Every resize of the window records its content size in [`RememberedSize`],
//!   with the screen it is on, unless the window is maximized, minimized or
//!   fullscreen then, so the last normal size is kept, as the plugin did.
//! - [`save`](fn@save) writes the last normal size as the app exits. It has
//!   one caller, `main`'s handler of `RunEvent::Exit`, which every way the app
//!   ends goes through: closing the window (the last window closing ends the
//!   app), Ctrl-Q on Linux (`quit_shortcut::install` calls `AppHandle::exit`),
//!   the erase's quit (`quit_after_erase`, which destroys the window first), a
//!   server error (`start_server`'s `exit(1)`) and a failed start's dialog
//!   (`show_startup_failure_dialog`). The window may already be gone by then,
//!   which is why the size is recorded as it changes rather than read at exit.
//!
//! The position is not remembered: Wayland does not let an app place its
//! window, so restoring a position would work on macOS and not on Linux, and
//! the compositor (or the first launch's centring) places it instead.

mod fit_to_screen;
use fit_to_screen::fit_to_screen;

mod fit_window_size;
pub use fit_window_size::fit_window_size;

mod follow_resizes;
use follow_resizes::follow_resizes;

mod initial_size;
use initial_size::initial_size;

mod read_remembered;
use read_remembered::read_remembered;

mod remembered_size;
pub use remembered_size::RememberedSize;

mod save;
pub use save::save;

mod saved_size;
use saved_size::SavedSize;

mod state_file;
use state_file::state_file;

mod window_mode;
use window_mode::WindowMode;

mod work_area;
use work_area::WorkArea;

mod write_remembered;
use write_remembered::write_remembered;

#[cfg(test)]
mod testing;

use tauri::LogicalSize;

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

/// The name of the file the size is kept in, in the app config directory.
pub const STATE_FILE_NAME: &str = "window-size.json";

/// Whether `size` can be a window's size: finite and not empty.
fn is_usable(size: LogicalSize<f64>) -> bool {
    [size.width, size.height]
        .iter()
        .all(|side| side.is_finite() && *side > 0.0)
}
