//! One-shot `claude --version` probe logged at startup.
//!
//! See `docs/guides/compatibility.md` (subdomain 3, "Startup version log") for
//! the rationale: a pure observability hook that records which upstream
//! `claude` binary delta-server is running against, so a later breakage report
//! can be correlated with a specific upstream version. There is no policy
//! enforcement — a missing or failing binary produces a warn-level log and
//! startup continues normally.
//!
//! The probe runs once, synchronously, before the HTTP server starts
//! listening, so the version line lands in the boot banner ahead of any
//! session activity.
//!
//! Honours `DELTA_CLAUDE_BIN` by accepting the resolved binary path from the
//! caller (which has already applied the env override).
//!
//! Kept in its own module so the spawn, output handling, and warn-on-failure
//! branches stay readable in isolation and can be unit-tested without booting
//! the full server.
//!
//! # Output format
//!
//! * **Success** (exit 0): info-level `claude --version: <trimmed stdout>`.
//! * **Spawn failed** (binary missing, permission denied, etc.): warn-level
//!   `failed to spawn '<bin> --version': <error>`.
//! * **Non-zero exit**: warn-level `'<bin> --version' exited <status>: <stderr>`.
//!
//! In every failure case the function returns normally — startup proceeds.
//!
//! # Why synchronous?
//!
//! The probe is a one-shot at boot, before the runtime takes on real work, so
//! a `std::process::Command::output()` is fine. No async runtime is needed.

use std::process::Command;

/// Spawn `<bin> --version` once and log the result. See the module docs for
/// the format and failure semantics.
///
/// Never panics, never returns an error: a spawn failure or non-zero exit is
/// logged at warn level and the caller continues.
pub fn log_claude_version(bin: &str) {
    let output = match Command::new(bin).arg("--version").output() {
        Ok(output) => output,
        Err(err) => {
            tracing::warn!(
                bin,
                error = %err,
                "failed to spawn '{bin} --version': {err}",
            );
            return;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!(
            bin,
            status = %output.status,
            stderr = %stderr.trim(),
            "'{bin} --version' exited {}: {}",
            output.status,
            stderr.trim(),
        );
        return;
    }

    let version = String::from_utf8_lossy(&output.stdout);
    let version = version.trim();
    tracing::info!(bin, version, "claude --version: {version}");
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    /// Write `body` (a `#!/bin/sh` script) to a temp file, mark it executable,
    /// and return the path. The temp file is leaked for the test's lifetime;
    /// the OS reaps `/tmp` later.
    fn write_stub(body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "delta-claude-version-stub-{}-{}.sh",
            std::process::id(),
            // Append a per-call discriminator so concurrent tests do not
            // collide on the same path.
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        let mut file = std::fs::File::create(&path).expect("create stub");
        file.write_all(body.as_bytes()).expect("write stub");
        let mut perms = std::fs::metadata(&path).expect("stat stub").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod stub");
        path
    }

    /// Success path: a stub that prints a known string on stdout and exits 0
    /// must be invoked without panicking. The visible behaviour for callers is
    /// "returns normally", which this asserts; the formatted info line is
    /// observed by the integration harness through `tracing-subscriber`.
    #[test]
    fn success_path_returns_without_panic() {
        let stub = write_stub("#!/bin/sh\necho 'claude 1.2.3 (Claude Code)'\n");
        // No assertion on log output — `tracing` captures depend on a global
        // subscriber that other tests in the binary may have installed. The
        // contract here is "does not panic, does not return an error".
        log_claude_version(stub.to_str().expect("utf-8 path"));
        let _ = std::fs::remove_file(&stub);
    }

    /// Missing-binary path: pointing at a path that does not exist must hit
    /// the warn-and-continue branch — no panic, no propagated error. This is
    /// what protects server startup when `claude` is not installed (e.g. in CI
    /// or when only `fake-claude` is available).
    #[test]
    fn missing_binary_does_not_panic() {
        log_claude_version("/does/not/exist/claude-binary-for-version-probe");
    }

    /// Non-zero exit: a stub that writes to stderr and exits non-zero must
    /// also be handled via the warn path, not propagated as an error.
    #[test]
    fn nonzero_exit_does_not_panic() {
        let stub = write_stub("#!/bin/sh\necho 'boom' 1>&2\nexit 2\n");
        log_claude_version(stub.to_str().expect("utf-8 path"));
        let _ = std::fs::remove_file(&stub);
    }
}
