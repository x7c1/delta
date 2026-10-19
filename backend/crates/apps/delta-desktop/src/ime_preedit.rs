//! Show the input method's uncommitted (preedit) text in the page on Linux.
//!
//! wry disables preedit on every WebKitGTK webview it builds
//! (`set_enable_preedit(false)` in its `webkitgtk` backend), to work around
//! fcitx placing its candidate window wrongly on older webkit2gtk
//! (tauri-apps/tauri#5986). With it disabled, text being composed — Japanese
//! before it is committed, for one — is never drawn in the text field, while
//! the candidate window still shows. WebKitGTK 2.50 and later place the
//! candidate window at the caret with preedit on, so the shell turns it back
//! on once the webview exists. Remove this once wry lets the option be set
//! (tauri-apps/wry#1724).

use tauri::{Runtime, WebviewWindow};
use webkit2gtk::{InputMethodContextExt, WebViewExt};

/// Turn preedit back on in `window`'s webview. The call runs on the main
/// thread after this returns; a webview without an input method context is
/// logged there and left as wry built it.
pub fn enable<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    window.with_webview(|webview| match webview.inner().input_method_context() {
        Some(context) => context.set_enable_preedit(true),
        None => tracing::warn!(
            "the webview has no input method context; uncommitted IME text stays hidden"
        ),
    })
}
