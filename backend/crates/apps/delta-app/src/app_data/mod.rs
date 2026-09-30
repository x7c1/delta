//! Where the desktop shell keeps its data.
//!
//! The CLI's defaults for the database (`delta.db`) and the per-spawn working
//! directories (`.tmp/session`) are relative to the working directory, which
//! for an app launched from Finder or a desktop file is `/` or `$HOME`. The
//! shell places both in the Tauri app data directory instead —
//! `~/Library/Application Support/io.github.x7c1.delta/` on macOS,
//! `~/.local/share/io.github.x7c1.delta/` on Linux. An explicit
//! `DELTA_DB_PATH` / `DELTA_SESSION_WORKDIR` still wins.
//!
//! The worktree base and the transcript root keep their `$HOME`-based defaults:
//! the transcript root is where Claude Code writes, not where Delta writes.

mod explicit_paths;
pub use explicit_paths::ExplicitPaths;

mod placement;
pub use placement::{place_in_app_data_dir, Placement};
