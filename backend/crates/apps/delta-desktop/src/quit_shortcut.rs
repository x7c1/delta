//! Ctrl-Q quits the app on Linux, unless the embedded terminal has focus.
//!
//! On macOS the default application menu's Quit item gives Cmd-Q; Tauri gives
//! Linux no menu, so the shell catches Ctrl+Q itself in the GTK window's
//! `key-press-event`, which runs before the webview sees the key.
//!
//! The terminal (xterm.js over the `/pty` bridge) forwards Ctrl-Q to the tmux
//! pane, where it is XON, so while the terminal has focus the key goes on to
//! the page and does not quit. The page tells the shell where focus is through
//! the document title: inside the Linux shell (which marks
//! `<html data-shell="tauri-linux">` before the page's scripts run) the terminal
//! appends [`TERMINAL_FOCUSED_TITLE_SUFFIX`] to the title while it has focus,
//! and the shell follows every title change. The title rather than a request
//! to the shell's `delta://` scheme, because the page's Content-Security-Policy
//! allows neither a `fetch` nor a frame to another origin; and rather than
//! asking the page through `eval` on each key press, because that answer comes
//! back after the key handler has had to return. The window's title is the
//! shell's own and does not follow the document's.

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::Arc;

#[cfg(target_os = "linux")]
use tauri::{Manager, Runtime, WebviewWindow, WebviewWindowBuilder};

/// The `data-shell` value the Linux shell marks the page with; the page's
/// `shell.ts` reads it.
const SHELL_MARKER: &str = "tauri-linux";

/// What the page appends to the document title while the terminal has focus;
/// the page's `shell.ts` writes it.
const TERMINAL_FOCUSED_TITLE_SUFFIX: &str = " [terminal focused]";

/// Runs at document start on every navigation, so the marker is set before
/// the page's own scripts run.
fn shell_marker_script() -> String {
    format!("document.documentElement.dataset.shell = '{SHELL_MARKER}';")
}

/// Whether the document `title` says the terminal has focus.
fn is_terminal_focused_title(title: &str) -> bool {
    title.ends_with(TERMINAL_FOCUSED_TITLE_SUFFIX)
}

/// Whether a key press quits: Ctrl with `q` or `Q` (Shift or Caps Lock), while
/// the terminal does not have focus. With the terminal focused the key is the
/// pane's.
fn is_quit(control: bool, key: Option<char>, terminal_focused: bool) -> bool {
    control && matches!(key, Some('q' | 'Q')) && !terminal_focused
}

/// Whether the page's terminal has focus, as the document title last said.
#[cfg(target_os = "linux")]
#[derive(Clone, Default)]
pub struct TerminalFocus(Arc<AtomicBool>);

#[cfg(target_os = "linux")]
impl TerminalFocus {
    fn set(&self, focused: bool) {
        self.0.store(focused, Ordering::Relaxed);
    }

    fn get(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Mark the page as inside the Linux shell and follow its title into `focus`.
#[cfg(target_os = "linux")]
pub fn style<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    focus: TerminalFocus,
) -> WebviewWindowBuilder<'a, R, M> {
    builder
        .initialization_script(shell_marker_script())
        .on_document_title_changed(move |_window, title| {
            focus.set(is_terminal_focused_title(&title));
        })
}

/// Quit on Ctrl+Q in the window unless `focus` says the terminal has it. Must
/// run on the main thread, after the window is built.
#[cfg(target_os = "linux")]
pub fn install<R: Runtime>(window: &WebviewWindow<R>, focus: TerminalFocus) -> anyhow::Result<()> {
    use gtk::gdk::ModifierType;
    use gtk::glib::Propagation;
    use gtk::prelude::WidgetExt;

    let app = window.app_handle().clone();
    window
        .gtk_window()?
        .connect_key_press_event(move |_, event| {
            let control = event.state().contains(ModifierType::CONTROL_MASK);
            if is_quit(control, event.keyval().to_unicode(), focus.get()) {
                tracing::info!("Ctrl-Q pressed outside the terminal; quitting");
                app.exit(0);
                Propagation::Stop
            } else {
                Propagation::Proceed
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_q_quits_when_the_terminal_does_not_have_focus() {
        assert!(is_quit(true, Some('q'), false));
        assert!(is_quit(true, Some('Q'), false));
    }

    #[test]
    fn ctrl_q_with_the_terminal_focused_is_not_a_quit() {
        assert!(!is_quit(true, Some('q'), true));
        assert!(!is_quit(true, Some('Q'), true));
    }

    #[test]
    fn other_keys_do_not_quit() {
        assert!(!is_quit(false, Some('q'), false));
        assert!(!is_quit(true, Some('w'), false));
        assert!(!is_quit(true, None, false));
    }

    #[test]
    fn the_title_suffix_marks_the_terminal_focused() {
        assert!(is_terminal_focused_title("Delta [terminal focused]"));
        assert!(!is_terminal_focused_title("Delta"));
        assert!(!is_terminal_focused_title(""));
    }

    #[test]
    fn the_marker_script_sets_the_linux_shell() {
        assert_eq!(
            shell_marker_script(),
            "document.documentElement.dataset.shell = 'tauri-linux';"
        );
    }
}
