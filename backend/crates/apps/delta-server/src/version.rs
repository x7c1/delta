//! Delta workspace version display.
//!
//! Renders the version string surfaced through `GET /api/version` (and hence
//! the browser's navigator footer). The base version comes from
//! `CARGO_PKG_VERSION`, injected at build time by cargo from
//! `backend/Cargo.toml`'s `[workspace.package].version`. Which form it takes
//! follows the build origin (`crate::config::build_origin`), not the build
//! profile: a `release` build (made by the bundle workflow) carries the plain
//! `v<version>`, and a `local` build (`make desktop`, `make dev`,
//! `cargo build`, in any profile) appends a `+dev.<short-sha>` suffix, so a
//! locally built app is distinguishable from the published bundle and from
//! another local build of a different commit.
//!
//! # Why `+dev` (build metadata), not `-dev` (pre-release)
//!
//! SemVer defines `-<tag>` as a pre-release identifier (an artifact ordered
//! *before* the base version), and `+<metadata>` as build metadata (same
//! precedence as the base version, adds identifying info). Delta's local
//! builds sit *after* the released `0.2.1` — they are the current tree at
//! that tagged version plus additional development commits — so the correct
//! SemVer construct is `+dev.<sha>`, not `-dev.<sha>`.
//!
//! The short sha is populated by `build.rs` into the `DELTA_GIT_SHA`
//! environment variable at compile time. When `git rev-parse` fails (no
//! `.git`, e.g. a tarball extract), the build script substitutes the literal
//! `unknown` so the string still renders (`v0.2.1+dev.unknown`) rather than
//! failing the build.
//!
//! The build origin is fixed at compile time, so a single binary always
//! reports the same form; [`render_version`] takes the origin as an argument
//! so tests cover both forms in one run.

use delta_bootstrap::BuildOrigin;

/// Workspace version from `backend/Cargo.toml` (`[workspace.package].version`).
///
/// Also what the release check compares the newest published release with
/// (see `delta_usecase::ReleaseCheck`): the base version, whatever the build
/// origin.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git sha of the checkout the binary was built from, or `unknown` if
/// `git rev-parse` was unavailable. Populated by `build.rs`.
const GIT_SHA: &str = env!("DELTA_GIT_SHA");

/// The version string the server exposes to the browser footer, for this
/// build's origin ([`crate::config::build_origin`]).
///
/// - `release` build: `v<version>` (e.g. `v0.2.1`).
/// - `local` build: `v<version>+dev.<short-sha>` (e.g. `v0.2.1+dev.a1b2c3d`).
pub fn display_version() -> String {
    render_version(VERSION, GIT_SHA, crate::config::build_origin())
}

/// The version string for `version` built from commit `sha` with `origin`,
/// in the forms [`display_version`] lists.
fn render_version(version: &str, sha: &str, origin: BuildOrigin) -> String {
    match origin {
        BuildOrigin::Release => format!("v{version}"),
        BuildOrigin::Local => format!("v{version}+dev.{sha}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_build_renders_the_plain_version() {
        assert_eq!(
            render_version("0.6.0", "a1b2c3d", BuildOrigin::Release),
            "v0.6.0"
        );
    }

    #[test]
    fn a_local_build_renders_the_version_with_the_commit() {
        assert_eq!(
            render_version("0.6.0", "a1b2c3d", BuildOrigin::Local),
            "v0.6.0+dev.a1b2c3d"
        );
    }
}
