//! The activation token a launcher hands Delta on Linux, and how a second
//! launch passes it on so the running window can come to the front.
//!
//! Wayland compositors only let a window take focus with an activation token
//! (focus-stealing prevention). The launcher — GNOME Shell's app grid, a
//! `.desktop` activation, a terminal using xdg-activation — hands the process
//! it starts a token in `XDG_ACTIVATION_TOKEN`, and in `DESKTOP_STARTUP_ID` for
//! X11's startup notification (GNOME sets both; GTK 3 reads only the latter).
//! When Delta already runs, though, the process that holds the token is the
//! *second* one, which `tauri_plugin_single_instance` stops: it forwards only
//! that process's `argv` and working directory to the running instance over
//! the session D-Bus, and exits. Without the token, the running window's
//! request to be focused is refused and GNOME shows a "Delta is ready"
//! notification instead.
//!
//! So the second launch leaves its token in a file in the runtime directory
//! ([`Handoff`]) before the plugin forwards the launch, and the running
//! instance's callback takes it from there and presents the window with it
//! ([`focus`]). A file, because the plugin offers no other way through: it
//! forwards the second process's real argv (`std::env::args()`), so an extra
//! argument would mean re-executing the process just to add one, and the
//! D-Bus interface is the plugin's own. The file is `0600` in the user's
//! `0700` runtime directory, written whole and renamed into place, and the
//! callback deletes it as it reads it. It is named after the app identifier,
//! as the plugin's D-Bus name is, so the dev build and the installed app do not
//! read each other's.
//!
//! Every launch takes the token out of its own environment first thing in
//! `main` ([`take_from_env`]), as the xdg-activation protocol asks of a client
//! that consumes it, so the commands Delta starts (tmux, the agents) do not
//! inherit a token already used. A launch that turns out to be the running
//! instance gives its token to GDK itself ([`claim`]), which is what GTK
//! would otherwise have done with `DESKTOP_STARTUP_ID`, and discards the file
//! it left.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

/// The variables a launcher passes the token in, the xdg-activation one first.
const TOKEN_VARS: [&str; 2] = ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"];

/// Take the launcher's activation token out of this process's environment.
///
/// Must run at the very start of `main`, before any thread starts: changing
/// the environment while another thread may read it (GTK, the tokio runtime,
/// the log writer) is unsound.
pub fn take_from_env() -> Option<String> {
    let token = token_in(|name| std::env::var(name).ok());
    for name in TOKEN_VARS {
        std::env::remove_var(name);
    }
    token
}

/// The token in an environment `var` reads: `XDG_ACTIVATION_TOKEN`, else
/// `DESKTOP_STARTUP_ID`. An empty value is no token.
fn token_in(var: impl Fn(&str) -> Option<String>) -> Option<String> {
    TOKEN_VARS
        .iter()
        .filter_map(|name| var(name))
        .find(|token| !token.is_empty())
}

/// The file a second launch leaves its token in for the running instance.
#[derive(Clone, Debug)]
pub struct Handoff {
    path: PathBuf,
}

impl Handoff {
    /// The hand-off file of the app `identifier`, in the runtime directory
    /// (`$XDG_RUNTIME_DIR`); `None` when there is none, and the running
    /// window is then focused without a token, as before.
    pub fn for_app(identifier: &str) -> Option<Self> {
        let dir = dirs::runtime_dir()?;
        Some(Self::at(dir.join(format!("{identifier}.activation-token"))))
    }

    fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// Leave `token` for the running instance. Written to a temporary file
    /// and renamed over the hand-off file, so a reader never sees half of it.
    pub fn leave(&self, token: &str) -> io::Result<()> {
        let mut temporary = OsString::from(self.path.as_os_str());
        temporary.push(format!(".{}.tmp", std::process::id()));
        let temporary = PathBuf::from(temporary);
        let written = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .and_then(|mut file| file.write_all(token.as_bytes()))
            .and_then(|()| fs::rename(&temporary, &self.path));
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        written
    }

    /// Take the token a second launch left, deleting the file. `None` when
    /// there is none: the launch had no token, or no hand-off file could be
    /// written.
    pub fn take(&self) -> Option<String> {
        let token = self.read()?;
        self.remove();
        Some(token).filter(|token| !token.is_empty())
    }

    /// Delete the hand-off file if it still holds `own_token`, the one this
    /// launch left before it turned out to be the running instance. A token
    /// a later launch has left since is kept for the callback.
    pub fn discard_own(&self, own_token: &str) {
        if self.read().as_deref() == Some(own_token) {
            self.remove();
        }
    }

    fn read(&self) -> Option<String> {
        match fs::read_to_string(&self.path) {
            Ok(token) => Some(token),
            Err(err) if err.kind() == io::ErrorKind::NotFound => None,
            Err(err) => {
                tracing::warn!(
                    path = %self.path.display(),
                    "could not read the activation token a second launch left: {err}"
                );
                // Not left for the next launch's callback to stumble on.
                self.remove();
                None
            }
        }
    }

    fn remove(&self) {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => tracing::warn!(
                path = %self.path.display(),
                "could not remove the activation token hand-off file: {err}"
            ),
        }
    }
}

/// The token this launch was started with, and where a second launch leaves
/// one.
#[derive(Debug)]
pub struct LaunchActivation {
    pub token: Option<String>,
    pub handoff: Option<Handoff>,
}

impl LaunchActivation {
    /// Leave this launch's token for the running instance, in case there is
    /// one. Called before the single-instance plugin forwards the launch;
    /// this launch does not know yet whether it is the second.
    pub fn leave_for_running_instance(&self) {
        let (Some(token), Some(handoff)) = (&self.token, &self.handoff) else {
            return;
        };
        if let Err(err) = handoff.leave(token) {
            tracing::warn!(
                path = %handoff.path.display(),
                "could not leave the activation token for a running instance: {err}"
            );
        }
    }
}

/// This launch is the running instance: discard the token file it left, and
/// give its token to GDK for the window it is about to open, as GTK would have
/// taken it from `DESKTOP_STARTUP_ID` itself. Runs on the main thread, once
/// the display is open.
pub fn claim(activation: LaunchActivation) {
    let Some(token) = activation.token else {
        return;
    };
    if let Some(handoff) = &activation.handoff {
        handoff.discard_own(&token);
    }
    set_startup_notification_id(&token);
}

/// Bring `window` to the front: with the `token` a second launch left when
/// there is one, else with a plain focus request, which a Wayland compositor
/// may refuse.
pub fn focus(window: &tauri::WebviewWindow, token: Option<String>) -> tauri::Result<()> {
    use gtk::prelude::GtkWindowExt;

    let Some(token) = token else {
        return window.set_focus();
    };
    let presented = window.clone();
    window.run_on_main_thread(move || {
        let gtk_window = match presented.gtk_window() {
            Ok(gtk_window) => gtk_window,
            Err(err) => {
                tracing::warn!("could not present the running window with its token: {err}");
                return;
            }
        };
        set_startup_notification_id(&token);
        // Focuses through GDK, which activates the surface with the token
        // set on the display just above.
        gtk_window.present();
    })
}

/// Set the token GDK presents the next window with: the xdg-activation token
/// on Wayland, the startup-notification ID on X11. The same calls GTK's own
/// `GtkApplication` makes before it forwards an `activate` to the running
/// instance.
fn set_startup_notification_id(token: &str) {
    use glib::prelude::*;
    use glib::translate::{from_glib, ToGlibPtr};

    let Some(gdk_display) = gtk::gdk::Display::default() else {
        tracing::warn!("no display to hand the activation token to");
        return;
    };
    if let Some(x11) = gdk_display.downcast_ref::<gdkx11::X11Display>() {
        x11.set_startup_notification_id(token);
        return;
    }
    // SAFETY: `gdk_wayland_display_get_type` only registers and returns the
    // type.
    let wayland_type: glib::Type =
        unsafe { from_glib(gdk_wayland_sys::gdk_wayland_display_get_type()) };
    if !gdk_display.type_().is_a(wayland_type) {
        tracing::warn!(
            display_type = %gdk_display.type_(),
            "the display is neither Wayland nor X11; the activation token is not used"
        );
        return;
    }
    let raw_display: *mut gtk::gdk::ffi::GdkDisplay = gdk_display.to_glib_none().0;
    let token = token.to_glib_none();
    // SAFETY: `raw_display` is a live `GdkWaylandDisplay` (checked above),
    // borrowed from `gdk_display` for the call, and `token` a NUL-terminated
    // string that outlives the call, which copies it.
    unsafe {
        gdk_wayland_sys::gdk_wayland_display_set_startup_notification_id(
            raw_display.cast(),
            token.0,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// What a second launch does with its environment, and what the running
    /// instance's callback then receives.
    fn hand_off(env: &HashMap<&str, &str>, handoff: &Handoff) -> Option<String> {
        let second_launch = LaunchActivation {
            token: token_in(|name| env.get(name).map(|value| value.to_string())),
            handoff: Some(handoff.clone()),
        };
        second_launch.leave_for_running_instance();
        handoff.take()
    }

    #[test]
    fn the_running_instance_receives_the_token_of_a_second_launch() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = Handoff::at(dir.path().join("app.activation-token"));
        let env = HashMap::from([("XDG_ACTIVATION_TOKEN", "wayland-token")]);

        assert_eq!(hand_off(&env, &handoff).as_deref(), Some("wayland-token"));
        assert!(
            !handoff.path.exists(),
            "the callback deletes the file as it reads it"
        );
        assert_eq!(handoff.take(), None, "a token is used once");
    }

    #[test]
    fn the_running_instance_receives_none_from_a_launch_without_a_token() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = Handoff::at(dir.path().join("app.activation-token"));

        assert_eq!(hand_off(&HashMap::new(), &handoff), None);
        assert_eq!(
            hand_off(&HashMap::from([("XDG_ACTIVATION_TOKEN", "")]), &handoff),
            None
        );
    }

    #[test]
    fn the_x11_startup_id_is_the_token_when_there_is_no_xdg_activation_token() {
        let x11 = HashMap::from([("DESKTOP_STARTUP_ID", "x11-id")]);
        assert_eq!(
            token_in(|name| x11.get(name).map(|value| value.to_string())).as_deref(),
            Some("x11-id")
        );
        let both = HashMap::from([
            ("XDG_ACTIVATION_TOKEN", "wayland-token"),
            ("DESKTOP_STARTUP_ID", "x11-id"),
        ]);
        assert_eq!(
            token_in(|name| both.get(name).map(|value| value.to_string())).as_deref(),
            Some("wayland-token")
        );
    }

    #[test]
    fn the_hand_off_file_is_owner_only_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = Handoff::at(dir.path().join("app.activation-token"));
        handoff.leave("token").unwrap();

        let mode = fs::metadata(&handoff.path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![OsString::from("app.activation-token")]);
    }

    #[test]
    fn the_running_instance_discards_only_its_own_token() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = Handoff::at(dir.path().join("app.activation-token"));

        handoff.leave("own").unwrap();
        handoff.discard_own("own");
        assert!(!handoff.path.exists());

        handoff.leave("a later launch's").unwrap();
        handoff.discard_own("own");
        assert_eq!(handoff.take().as_deref(), Some("a later launch's"));
    }

    #[test]
    fn taking_the_token_removes_it_from_the_environment() {
        // The only test that touches these variables; std serialises its own
        // environment access, and nothing else in the test process reads them.
        std::env::set_var("XDG_ACTIVATION_TOKEN", "wayland-token");
        std::env::set_var("DESKTOP_STARTUP_ID", "x11-id");

        assert_eq!(take_from_env().as_deref(), Some("wayland-token"));
        for name in TOKEN_VARS {
            assert_eq!(std::env::var_os(name), None, "{name} is left behind");
        }
        assert_eq!(take_from_env(), None);
    }
}
