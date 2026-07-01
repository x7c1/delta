//! Build-time git sha probe.
//!
//! Records the short git sha of the current checkout into `DELTA_GIT_SHA` so
//! debug builds can embed it in the version string surfaced to the browser
//! (`v0.2.1+dev.a1b2c3d`). Falls back to the literal `unknown` if `git
//! rev-parse` is unavailable (e.g. a tarball extract) so a source-only build
//! never fails here.
//!
//! Rerun-if-changed on `.git/HEAD` picks up branch switches and new commits
//! without touching the crate sources; cargo still won't rebuild for every
//! commit in a working tree with no source changes, and that is fine — the
//! sha is a debugging hint, not a fingerprint.

fn main() {
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=DELTA_GIT_SHA={sha}");
    println!("cargo:rerun-if-changed=.git/HEAD");
}
