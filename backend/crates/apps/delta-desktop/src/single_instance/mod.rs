//! Delta's own single-instance lock, used on macOS.
//!
//! `tauri-plugin-single-instance` keeps its macOS socket at a fixed path in
//! `/tmp`, which every user of the Mac can write: another user could take the
//! path first and make Delta exit on every launch, or make it skip the check
//! and run twice against the same database and tmux socket. On macOS the shell
//! therefore keeps its own lock: a Unix socket named after the app identifier
//! (`socket_path.rs`) in the per-user temporary directory macOS assigns
//! (`user_temp_dir.rs`), which only the user can write (`ensure_private_dir.rs`).
//! On Linux the plugin stays: it uses the session D-Bus, which is per user.
//!
//! `claim` behaves as the plugin did: a launch that reaches a running copy
//! tells it and exits; one that finds no socket, or a stale one a crashed copy
//! left (nothing listening), takes the path and listens. The running copy
//! reads each connection on a thread of its own, for at most `READ_LIMIT`,
//! so a client that connects and never sends or closes cannot hold up the
//! next launch. The lock is released when the app exits (`plugin.rs`).
//!
//! When the lock cannot be set up, the app logs why and launches without it,
//! as the plugin did; it never falls back to `/tmp`.
//!
//! Everything but the directory's choice and the wiring into Tauri works on
//! any Unix, so it is tested on Linux too.

mod claim;
mod ensure_private_dir;
mod instance_lock;
#[cfg(target_os = "macos")]
mod plugin;
#[cfg(target_os = "macos")]
pub use plugin::init;
mod socket_path;
#[cfg(target_os = "macos")]
mod user_temp_dir;
