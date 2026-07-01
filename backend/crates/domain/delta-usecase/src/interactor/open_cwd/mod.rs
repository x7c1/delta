//! `open_cwd`: launch an external tool (VS Code) against a session's cwd.
//!
//! The endpoint takes a path plus an optional handler id and:
//!
//! 1. Rejects the path if it is not in the known-cwd allowlist (defence in
//!    depth so a hand-crafted request cannot open an arbitrary directory).
//! 2. Resolves the handler from the internal registry (defaults to VS Code).
//! 3. Spawns the handler's command with the path as a single argument via the
//!    [`ExternalOpener`] port.
//!
//! Only one handler ships initially — VS Code — but the shape is a registry
//! rather than a hard-coded call so a future entry (IntelliJ, Zed, …) only
//! adds a row to the table.

use crate::error::{Error, Result};
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

/// Stable id of the VS Code handler, matching the `handler` value the browser
/// sends when it explicitly picks a handler (`{ path, handler: "vscode" }`).
/// It is also the default when the request omits `handler`, so an untyped
/// request opens VS Code.
pub const VSCODE_HANDLER_ID: &str = "vscode";

/// Alias for the string id keying a handler in the registry.
///
/// A newtype would let the shape carry more compile-time meaning, but a
/// stringly-typed id keeps the wire mapping trivial (the browser sends an
/// arbitrary string; the interactor either finds it in the registry or
/// rejects with [`Error::OpenCwdUnknownHandler`]). Keeping it as `String`
/// alias also matches the browser-facing `handler` field.
pub type ExternalHandlerId = &'static str;

/// One handler in the registry: the id the browser picks by, a human-facing
/// display name for menus, and the command Delta shells out to.
///
/// The initial registry contains one entry (VS Code); adding another handler
/// is a single [`registry`] row. Argument rendering is intentionally minimal
/// — every current handler takes the target path as a single trailing
/// argument, so a `Vec<String>` built from `[path]` covers it. If a future
/// handler needs more shape (a `--project` flag, multiple positional args),
/// the registry entry can grow a rendering closure without disturbing the
/// call site.
#[derive(Debug, Clone, Copy)]
pub struct ExternalHandler {
    /// Stable id the browser uses to pick this handler
    /// (`{ handler: "<id>" }`). Also acts as the registry key.
    pub id: ExternalHandlerId,
    /// Human-facing name for menus and error messages (`Visual Studio Code`).
    pub display_name: &'static str,
    /// The command Delta invokes (e.g. `code`). Passed to
    /// [`ExternalOpener::open`] as-is; the path is appended as the sole arg.
    pub command: &'static str,
}

/// The registered handlers. Initially just VS Code — the abstraction is in
/// place for a future entry to slot in without touching the endpoint.
///
/// `code <path>` is deliberate: it opens the path in a *new* window (or
/// focuses the existing one that already has it). `code --add` would target
/// the last-active VS Code window, which the user cannot control.
const REGISTRY: &[ExternalHandler] = &[ExternalHandler {
    id: VSCODE_HANDLER_ID,
    display_name: "Visual Studio Code",
    command: "code",
}];

/// Look up a handler by id, returning `None` when the id is not registered.
fn find_handler(id: &str) -> Option<ExternalHandler> {
    REGISTRY.iter().copied().find(|h| h.id == id)
}

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Open `path` in an external tool. When `handler_id` is `None`, the
    /// default handler (VS Code) is used; otherwise it must resolve to a
    /// registered handler or [`Error::OpenCwdUnknownHandler`] is returned.
    ///
    /// `path` must appear in the known-cwd allowlist — the set of paths
    /// Delta has actually shown the browser (`session.cwd`,
    /// `session.requested_workdir`, or `message.cwd`). A path not in the set
    /// is [`Error::OpenCwdPathNotAllowed`], not silently spawned: the click
    /// site never sends a path the server hasn't shown it, so this only
    /// fires against a hand-crafted request.
    pub async fn open_cwd(&self, path: &str, handler_id: Option<&str>) -> Result<()> {
        let handler = match handler_id {
            Some(id) => {
                find_handler(id).ok_or_else(|| Error::OpenCwdUnknownHandler(id.to_owned()))?
            }
            None => find_handler(VSCODE_HANDLER_ID)
                .expect("the default VS Code handler is always registered"),
        };
        if !self.store.cwd_exists(path).await? {
            return Err(Error::OpenCwdPathNotAllowed(path.to_owned()));
        }
        self.external_opener
            .open(handler.command, vec![path.to_owned()])
            .await
    }
}

#[cfg(test)]
mod tests;
