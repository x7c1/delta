//! The hook state file: what keeps a session's hook URLs valid across restarts.
//!
//! A Claude Code session runs in tmux and outlives the Delta process that
//! launched it, and it keeps calling the hook URLs rendered into its settings
//! file (`http://127.0.0.1:<port>/hooks/<event>?hs=<secret>`) until it exits.
//! For those calls to reach — and be accepted by — the next Delta process, the
//! port and the hook secret have to survive the restart. This file records
//! them.
//!
//! It is `delta-hook-state.json` in the server's data directory
//! ([`DataLayout::hook_state`](delta_bootstrap::DataLayout::hook_state)), so
//! the installed app, the desktop dev build and `make dev` each get their own.
//! It holds a JSON object with:
//!
//! - `hook_secret` — the secret every hook URL carries. Read on start; minted
//!   and recorded when absent. `DELTA_HOOK_SECRET` overrides it and is never
//!   recorded.
//! - `port` — the port the desktop app last chose for itself. The app tries it
//!   first on the next launch (see [`crate::serve::bind_app_listener`]). An
//!   explicit `DELTA_PORT` is never recorded, and the CLI server, whose port is
//!   fixed, never reads or writes it.
//!
//! The file is owner-only (0600) because the secret authenticates hook calls.
//! A file found with looser permissions is tightened to 0600 on start, with a
//! warning, rather than refused: the secret it holds may already have been
//! readable, but refusing to start would not un-read it, and deleting the file
//! is how to rotate the secret. A missing file is a first run; a file that is
//! not valid JSON, or holds a secret that cannot ride a URL, is ignored with a
//! warning and overwritten with a fresh secret.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::secrets::mint_secret;

#[cfg(test)]
mod tests;

/// Owner read/write only: the file holds the hook secret.
const STATE_FILE_MODE: u32 = 0o600;

/// The permission bits that must be clear on the state file: everything for
/// the group and for others.
const NON_OWNER_BITS: u32 = 0o077;

/// A failure to read or write the hook state file.
///
/// Fatal at startup: the server cannot know which secret to accept without the
/// file, and a fresh secret it cannot record would silently change on every
/// restart. Each variant names the path, since the remedy — fixing the
/// directory's permissions, or deleting the file — is the user's.
#[derive(Debug, thiserror::Error)]
pub enum HookStateError {
    /// The file exists but could not be read.
    #[error(
        "could not read the hook state file {}: {source}. Fix its permissions, \
         or delete it to have Delta create a new one",
        path.display()
    )]
    Read {
        /// The state file.
        path: PathBuf,
        /// What reading it failed with.
        source: std::io::Error,
    },
    /// The file was found readable by others and could not be restricted to
    /// its owner.
    #[error(
        "could not restrict the hook state file {} to its owner: {source}. \
         Delete it to have Delta create a new one",
        path.display()
    )]
    Restrict {
        /// The state file.
        path: PathBuf,
        /// What changing its permissions failed with.
        source: std::io::Error,
    },
    /// The file could not be written.
    #[error("could not write the hook state file {}: {source}", path.display())]
    Write {
        /// The state file.
        path: PathBuf,
        /// What writing it failed with.
        source: std::io::Error,
    },
}

/// The hook secret a start settled on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettledSecret {
    /// The secret this run renders into hook URLs and accepts on them.
    pub secret: String,
    /// Whether it differs from the secret the state file recorded — or there
    /// was none to compare with — so hook URLs a previous run rendered carry a
    /// secret this run refuses.
    pub changed: bool,
}

/// The contents of the state file. Both fields are optional so either can be
/// recorded without the other.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Recorded {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hook_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
}

/// The hook state file, as read at startup.
#[derive(Debug)]
pub struct HookStateFile {
    path: PathBuf,
    recorded: Recorded,
}

impl HookStateFile {
    /// Read the state file at `path`.
    ///
    /// A missing file reads as empty. A file with permissions looser than 0600
    /// is tightened first (see the module docs).
    pub fn open(path: PathBuf) -> Result<Self, HookStateError> {
        let Some(text) = read_private(&path)? else {
            return Ok(Self {
                path,
                recorded: Recorded::default(),
            });
        };
        let recorded = match serde_json::from_str::<Recorded>(&text) {
            Ok(recorded) => sanitize(recorded, &path),
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    "the hook state file is not valid JSON ({err}); \
                     it will be overwritten and the hook secret rotated"
                );
                Recorded::default()
            }
        };
        Ok(Self { path, recorded })
    }

    /// Where the state file lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The port the desktop app recorded on its last launch, if any.
    pub fn port(&self) -> Option<u16> {
        self.recorded.port
    }

    /// Settle this run's hook secret.
    ///
    /// A non-empty `explicit` secret (`DELTA_HOOK_SECRET`) wins and is not
    /// recorded. Otherwise the recorded secret is reused, or, when there is
    /// none, a fresh one is minted and recorded.
    pub fn settle_hook_secret(
        &mut self,
        explicit: Option<String>,
    ) -> Result<SettledSecret, HookStateError> {
        if let Some(secret) = explicit.filter(|secret| !secret.is_empty()) {
            let changed = self.recorded.hook_secret.as_deref() != Some(secret.as_str());
            return Ok(SettledSecret { secret, changed });
        }
        if let Some(secret) = &self.recorded.hook_secret {
            return Ok(SettledSecret {
                secret: secret.clone(),
                changed: false,
            });
        }
        let secret = mint_secret();
        self.write(Recorded {
            hook_secret: Some(secret.clone()),
            ..self.recorded.clone()
        })?;
        tracing::info!(
            path = %self.path.display(),
            "minted a hook secret and recorded it"
        );
        Ok(SettledSecret {
            secret,
            changed: true,
        })
    }

    /// Record `port` as the one the desktop app tries first next launch. A
    /// no-op when it is already the recorded one.
    pub fn record_port(&mut self, port: u16) -> Result<(), HookStateError> {
        if self.recorded.port == Some(port) {
            return Ok(());
        }
        self.write(Recorded {
            port: Some(port),
            ..self.recorded.clone()
        })
    }

    /// Replace the file's contents with `recorded`, owner-only.
    ///
    /// Written to a sibling temporary file and renamed over the state file, so
    /// a crash mid-write leaves the previous contents rather than a truncated
    /// file that would rotate the secret on the next start.
    fn write(&mut self, recorded: Recorded) -> Result<(), HookStateError> {
        let write_error = |source| HookStateError::Write {
            path: self.path.clone(),
            source,
        };
        let json = serde_json::to_string_pretty(&recorded)
            .map_err(|err| write_error(std::io::Error::other(err)))?;
        let mut temp_name = self.path.as_os_str().to_owned();
        temp_name.push(".tmp");
        let temp = PathBuf::from(temp_name);
        write_private(&temp, json.as_bytes()).map_err(write_error)?;
        fs::rename(&temp, &self.path).map_err(write_error)?;
        self.recorded = recorded;
        Ok(())
    }
}

/// Read `path` as text, tightening its permissions to 0600 first when the group
/// or others have any. `None` when there is no file.
fn read_private(path: &Path) -> Result<Option<String>, HookStateError> {
    let read_error = |source| HookStateError::Read {
        path: path.to_owned(),
        source,
    };
    let mode = match fs::metadata(path) {
        Ok(meta) => meta.permissions().mode(),
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(read_error(err)),
    };
    if mode & NON_OWNER_BITS != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(STATE_FILE_MODE)).map_err(
            |source| HookStateError::Restrict {
                path: path.to_owned(),
                source,
            },
        )?;
        tracing::warn!(
            path = %path.display(),
            mode = format_args!("{:o}", mode & 0o777),
            "the hook state file was readable beyond its owner; restricted it to 0600. \
             Delete it to rotate the hook secret"
        );
    }
    fs::read_to_string(path).map(Some).map_err(read_error)
}

/// Drop recorded values this run cannot use, warning about each: a secret that
/// is empty or would not survive unescaped in a URL query, and port 0.
fn sanitize(mut recorded: Recorded, path: &Path) -> Recorded {
    if let Some(secret) = &recorded.hook_secret {
        if secret.is_empty() || !secret.chars().all(|c| c.is_ascii_alphanumeric()) {
            tracing::warn!(
                path = %path.display(),
                "the hook state file holds an unusable hook secret; minting a new one"
            );
            recorded.hook_secret = None;
        }
    }
    if recorded.port == Some(0) {
        recorded.port = None;
    }
    recorded
}

/// Write `bytes` to `path` as an owner-only file, replacing any file there.
///
/// `mode` applies only when the file is created, so the explicit
/// `set_permissions` also tightens a leftover temporary file.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(STATE_FILE_MODE)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(STATE_FILE_MODE))?;
    file.write_all(bytes)?;
    file.sync_all()
}
