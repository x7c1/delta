//! The macOS window's transparent title bar.
//!
//! The window uses [`TitleBarStyle::Overlay`] with a hidden title: the bar is
//! transparent, the page is laid out under it, and the traffic lights float
//! over the page's own themed background. An initialization script marks
//! `<html data-shell="tauri-macos">` and sets `--shell-top-inset` on it to
//! [`TITLE_BAR_HEIGHT`], so the page reserves exactly the strip the bar and
//! the drag view cover.
//!
//! With the page under the bar the webview receives the bar's mouse events, so
//! the native bar would neither drag nor zoom. [`install_drag_strip`] puts a
//! transparent view over the strip, in front of the webview and behind the
//! traffic lights, that drags the window and handles double-click.
//!
//! In full screen the bar is hidden, so the inset drops to 0 and the view lets
//! clicks through: the page gets the whole screen, as other macOS apps do.

use objc2::rc::Retained;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSEvent, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{ns_string, NSPoint, NSRect, NSSize, NSUserDefaults};
use tauri::webview::PageLoadEvent;
use tauri::{Manager, Runtime, TitleBarStyle, WebviewWindow, WebviewWindowBuilder, WindowEvent};

/// Height of a macOS title bar with a hidden title, in points (CSS pixels at
/// the webview's scale). The drag view and the page's inset both use it.
const TITLE_BAR_HEIGHT: f64 = 28.0;

/// Runs at document start on every navigation of the webview, external URLs
/// included, so the marker and the inset are set before the page's own
/// scripts run. It assumes a windowed window; [`sync_inset`] corrects the
/// inset once the page has loaded in full screen.
fn shell_marker_script() -> String {
    format!(
        "document.documentElement.dataset.shell = 'tauri-macos';{}",
        inset_script(TITLE_BAR_HEIGHT)
    )
}

fn inset_script(height: f64) -> String {
    format!("document.documentElement.style.setProperty('--shell-top-inset', '{height}px');")
}

/// Set the page's inset to what the window shows now: the bar's height when
/// windowed, 0 in full screen, where the bar is hidden.
fn sync_inset<R: Runtime>(window: &WebviewWindow<R>) {
    let full_screen = window.is_fullscreen().unwrap_or_else(|err| {
        tracing::warn!("could not read the window's full-screen state: {err}");
        false
    });
    let height = if full_screen { 0.0 } else { TITLE_BAR_HEIGHT };
    if let Err(err) = window.eval(inset_script(height)) {
        tracing::warn!("could not update the title-bar inset: {err}");
    }
}

/// Apply the transparent title bar and the shell marker to the window builder.
pub fn style<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    builder
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        .initialization_script(shell_marker_script())
        // A reload in full screen starts from the windowed inset again.
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                sync_inset(&window);
            }
        })
}

/// Cover the title-bar strip with a view that drags and zooms the window.
/// Must run on the main thread, after the window (and its webview) is built,
/// so the view lands in front of the webview.
pub fn install_drag_strip<R: Runtime>(window: &WebviewWindow<R>) -> anyhow::Result<()> {
    let mtm = MainThreadMarker::new().ok_or_else(|| {
        anyhow::anyhow!("the title-bar strip must be installed on the main thread")
    })?;
    let ns_window = window.ns_window()?;
    // SAFETY: Tauri hands out the live `NSWindow` of a window it has built;
    // it stays alive for as long as `window` does, which outlives this call.
    let ns_window: &NSWindow = unsafe { &*ns_window.cast() };
    let content = ns_window
        .contentView()
        .ok_or_else(|| anyhow::anyhow!("the window has no content view"))?;

    let bounds = content.bounds();
    // Pin the strip to the top edge in either coordinate orientation.
    let (y, top_margin) = if content.isFlipped() {
        (0.0, NSAutoresizingMaskOptions::ViewMaxYMargin)
    } else {
        (
            bounds.size.height - TITLE_BAR_HEIGHT,
            NSAutoresizingMaskOptions::ViewMinYMargin,
        )
    };
    let frame = NSRect::new(
        NSPoint::new(0.0, y),
        NSSize::new(bounds.size.width, TITLE_BAR_HEIGHT),
    );
    let strip = TitleBarDragView::new(mtm, frame);
    strip.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | top_margin);
    content.addSubview(&strip);

    // Entering and leaving full screen resize the window.
    let observed = window.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Resized(_)) {
            sync_inset(&observed);
        }
    });
    Ok(())
}

define_class!(
    /// A transparent view that forwards title-bar clicks to its window.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "DeltaTitleBarDragView"]
    struct TitleBarDragView;

    impl TitleBarDragView {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let Some(window) = self.window() else {
                return;
            };
            if event.clickCount() == 2 {
                on_double_click(&window);
            } else {
                window.performWindowDragWithEvent(event);
            }
        }

        // In full screen the bar is hidden and the page is laid out under the
        // strip, so its clicks go to the page.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            let full_screen = self
                .window()
                .is_some_and(|window| window.styleMask().contains(NSWindowStyleMask::FullScreen));
            if full_screen {
                None
            } else {
                // SAFETY: forwards to NSView's own `hitTest:` with the same
                // argument and return type.
                unsafe { msg_send![super(self), hitTest: point] }
            }
        }

        // A native title bar drags an inactive window on the first click.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }
    }
);

impl TitleBarDragView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // subclass adds no state of its own.
        unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] }
    }
}

/// Follow System Settings > Desktop & Dock > "Double-click a window's title
/// bar to": minimize, do nothing, or (the default) zoom.
fn on_double_click(window: &NSWindow) {
    let action = NSUserDefaults::standardUserDefaults()
        .stringForKey(ns_string!("AppleActionOnDoubleClick"))
        .map(|action| action.to_string());
    match action.as_deref() {
        Some("Minimize") => window.miniaturize(None),
        Some("None") => {}
        _ => window.performZoom(None),
    }
}
