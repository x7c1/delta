//! Delta workspace version display.
//!
//! Renders the version string surfaced through `GET /api/version` (and hence
//! the browser's navigator footer). The base version comes from
//! `CARGO_PKG_VERSION`, injected at build time by cargo from
//! `backend/Cargo.toml`'s `[workspace.package].version`. Debug builds append a
//! `+dev.<short-sha>` suffix so a running dev build is distinguishable from a
//! released tag; release builds carry the plain `v<version>`.
//!
//! # Why `+dev` (build metadata), not `-dev` (pre-release)
//!
//! SemVer defines `-<tag>` as a pre-release identifier (an artifact ordered
//! *before* the base version), and `+<metadata>` as build metadata (same
//! precedence as the base version, adds identifying info). Delta's debug
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
//! The `debug_assertions` cfg is compile-time, so a single binary always
//! reports the same format — either the plain or the sha-tagged one. Tests
//! therefore only exercise the profile they run under and assert the shape,
//! not both branches.

/// Workspace version from `backend/Cargo.toml` (`[workspace.package].version`).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git sha of the checkout the binary was built from, or `unknown` if
/// `git rev-parse` was unavailable. Populated by `build.rs`.
const GIT_SHA: &str = env!("DELTA_GIT_SHA");

/// The version string the server exposes to the browser footer.
///
/// - Release build: `v<version>` (e.g. `v0.2.1`).
/// - Debug build:  `v<version>+dev.<short-sha>` (e.g. `v0.2.1+dev.a1b2c3d`).
pub fn display_version() -> String {
    if cfg!(debug_assertions) {
        format!("v{VERSION}+dev.{GIT_SHA}")
    } else {
        format!("v{VERSION}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rendered version starts with `v` followed by the workspace version.
    /// Both build profiles satisfy this shared prefix, which is all a runtime
    /// test can pin — the debug/release branch is compile-time.
    #[test]
    fn display_starts_with_v_and_the_workspace_version() {
        let rendered = display_version();
        assert!(
            rendered.starts_with(&format!("v{VERSION}")),
            "expected prefix v{VERSION}, got {rendered}",
        );
    }

    /// A debug build additionally carries the `+dev.<sha>` build metadata
    /// suffix (SemVer build metadata, not a pre-release). Release-only test
    /// runs skip this assertion — `cargo test` uses the debug profile by
    /// default, so this branch is what a normal `make test` exercises.
    #[test]
    #[cfg(debug_assertions)]
    fn debug_display_carries_a_dev_metadata_suffix() {
        let rendered = display_version();
        // `+dev.` is the literal marker; the sha is `unknown` when the build
        // ran without a `.git`, and a hex short sha otherwise — both are
        // non-empty by construction, so assert the marker and a non-empty
        // tail.
        let (base, tail) = rendered
            .split_once("+dev.")
            .expect("debug build must carry `+dev.<sha>` metadata");
        assert_eq!(base, format!("v{VERSION}"));
        assert!(
            !tail.is_empty(),
            "the sha tail must be populated (fallback: `unknown`)",
        );
    }

    /// A release build has no metadata suffix — just `v<version>` — so a `+`
    /// character MUST NOT appear anywhere in the rendered string. This is the
    /// mirror of the debug assertion, guarded by the opposite cfg.
    #[test]
    #[cfg(not(debug_assertions))]
    fn release_display_has_no_metadata_suffix() {
        let rendered = display_version();
        assert!(
            !rendered.contains('+'),
            "release build must not carry `+dev.<sha>`, got {rendered}",
        );
        assert_eq!(rendered, format!("v{VERSION}"));
    }
}
