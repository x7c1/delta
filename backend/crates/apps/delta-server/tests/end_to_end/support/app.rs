//! Assembling the app with test-wired gateways.

use std::sync::Arc;

use axum::Router;

use delta_server::{router, AppState, StorageInventory};
use delta_sqlite::SqliteStore;
use delta_transcript::JsonlTranscript;
use delta_usecase::{Interactor, TmuxDriver};

use super::{FakeTmux, NoopGitWorktree, NoopWorkspace, SharedTmux, AUTH_TOKEN, HOOK_SECRET};

/// Assemble the app with test-wired gateways and return the router plus the
/// fake tmux driver (for asserting keystroke dispatch) and the transcript path.
pub(crate) fn build_app() -> (Router, Arc<FakeTmux>, std::path::PathBuf, AppState) {
    build_app_with_store(SqliteStore::open_in_memory().unwrap())
}

/// [`build_app`] over a store the caller has already seeded, for a test that
/// needs rows the REST surface has no direct way to create.
pub(crate) fn build_app_with_store(
    store: SqliteStore,
) -> (Router, Arc<FakeTmux>, std::path::PathBuf, AppState) {
    let transcript_file = tempfile::Builder::new()
        .prefix("delta-e2e-transcript-")
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    // Keep the path but let the file persist for the whole test.
    let (_, transcript_path) = transcript_file.keep().unwrap();

    let tmux = Arc::new(FakeTmux::default());
    let transcript = JsonlTranscript::new();

    let interactor = Interactor::new(
        Box::new(SharedTmux(tmux.clone())) as Box<dyn TmuxDriver>,
        Box::new(transcript) as Box<dyn delta_usecase::Transcript>,
        Box::new(store) as Box<dyn delta_usecase::SessionStore>,
        Box::new(NoopWorkspace) as Box<dyn delta_usecase::Workspace>,
        Box::new(NoopGitWorktree) as Box<dyn delta_usecase::GitWorktree>,
        "/tmp/delta-e2e-worktrees",
        "{}",
        "/tmp/delta-e2e-settings.json",
    );

    let storage = StorageInventory::new(
        "delta-e2e",
        "/tmp/delta-e2e-data",
        7878,
        "/tmp/delta-e2e-worktrees",
        "/tmp",
    );
    let state =
        AppState::from_interactor(interactor, "delta-e2e", AUTH_TOKEN, HOOK_SECRET, storage);
    (router(state.clone()), tmux, transcript_path, state)
}
