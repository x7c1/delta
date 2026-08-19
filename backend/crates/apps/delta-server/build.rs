//! Build-time git sha probe.
//!
//! Records the short git sha of the current checkout into `DELTA_GIT_SHA` so
//! debug builds can embed it in the version string surfaced to the browser
//! (`v0.2.1+dev.a1b2c3d`). Falls back to the literal `unknown` if `git
//! rev-parse` is unavailable (e.g. a tarball extract) so a source-only build
//! never fails here.
//!
//! Rerun-if-changed on the repository's `HEAD` file picks up branch switches
//! without touching the crate sources; cargo still won't rebuild for every
//! commit in a working tree with no source changes, and that is fine — the
//! sha is a debugging hint, not a fingerprint.
//!
//! The `HEAD` path must come from `git rev-parse --git-path` at build time:
//! relative `rerun-if-changed` paths resolve against the crate directory, but
//! the `.git` directory lives at the repository root (or elsewhere entirely in
//! a linked worktree). Registering a path that does not exist makes cargo
//! treat the crate as permanently stale and rebuild it on every invocation.

fn main() {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let sha = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=DELTA_GIT_SHA={sha}");
    if let Some(head) = git(&["rev-parse", "--path-format=absolute", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
}
