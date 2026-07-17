//! [`PathBinaryDetector`]: the concrete [`BinaryDetector`].
//!
//! Resolves a launch binary the same way spawn's `Command::new(bin)` would — a
//! bare command name looked up on `PATH`, or an explicit path checked for
//! existence and execute permission — so the provider-availability endpoint's
//! verdict matches what a real launch attempt would resolve. The answer is
//! memoised per binary for the process lifetime (binary presence effectively
//! does not change while the server runs), mirroring the `gh auth status` probe.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;

use delta_usecase::BinaryDetector;

/// Resolves launch binaries against the real filesystem / `PATH`, memoising
/// each answer for the process lifetime.
#[derive(Debug, Default)]
pub struct PathBinaryDetector {
    /// Per-binary presence memo. `std::sync::Mutex` (not tokio) because the
    /// lookup is a synchronous filesystem probe with no await held across the
    /// lock.
    cache: Mutex<HashMap<String, bool>>,
}

impl PathBinaryDetector {
    /// Build a fresh detector with an empty cache.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl BinaryDetector for PathBinaryDetector {
    async fn is_available(&self, bin: &str) -> bool {
        if let Some(hit) = self.cache.lock().unwrap().get(bin).copied() {
            return hit;
        }
        let resolved = resolve(bin);
        if !resolved {
            tracing::debug!(binary = %bin, "launch binary not found; provider reported unavailable");
        }
        self.cache.lock().unwrap().insert(bin.to_owned(), resolved);
        resolved
    }
}

/// Whether `bin` resolves to an executable file, mirroring how
/// `Command::new(bin)` would find it.
///
/// A name containing a path separator (or an absolute path) is treated as an
/// explicit location and checked directly; a bare name is searched across every
/// `PATH` entry. An empty name never resolves.
fn resolve(bin: &str) -> bool {
    if bin.is_empty() {
        return false;
    }

    let path = Path::new(bin);
    if path.is_absolute() || path.components().count() > 1 {
        return is_executable_file(path);
    }

    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| is_executable_file(&dir.join(bin)))
}

/// Whether `path` is an existing, regular, executable file.
#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        // Require the execute bit for *someone* (owner/group/other): a spawn of
        // a non-executable file would fail, so it is not "available".
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Whether `path` is an existing regular file. On non-Unix the execute bit is
/// not modelled the same way, so file existence is the closest portable proxy.
#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

    #[test]
    fn empty_name_never_resolves() {
        assert!(!resolve(""));
    }

    #[test]
    fn absolute_path_to_a_missing_file_is_absent() {
        assert!(!resolve("/definitely/not/here/codex-xyz"));
    }

    #[cfg(unix)]
    #[test]
    fn absolute_path_to_an_executable_is_present() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("codex");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        make_executable(&bin);
        assert!(resolve(bin.to_str().unwrap()));
    }

    #[cfg(unix)]
    #[test]
    fn absolute_path_to_a_non_executable_file_is_absent() {
        // Present on disk but without an execute bit: a spawn would fail, so it
        // must not be reported available.
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("codex");
        std::fs::write(&bin, "not executable").unwrap();
        assert!(!resolve(bin.to_str().unwrap()));
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_is_not_an_executable_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!resolve(dir.path().to_str().unwrap()));
    }

    #[cfg(unix)]
    #[test]
    fn bare_name_is_found_on_path() {
        // Put a fake `codex` on a temp PATH and probe by bare name. Isolating
        // PATH to a temp dir keeps the test independent of the host's tools.
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("codex-fake-bin");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        make_executable(&bin);

        // `resolve` reads the process PATH; scope the override to this test.
        let original = std::env::var_os("PATH");
        std::env::set_var("PATH", dir.path());
        let found = resolve("codex-fake-bin");
        let missing = resolve("definitely-not-on-path-xyz");
        match original {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }

        assert!(found, "a bare name present on PATH resolves");
        assert!(!missing, "a bare name absent from PATH does not resolve");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn detector_caches_and_reports_presence() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("codex");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        make_executable(&bin);
        let path = bin.to_str().unwrap();

        let detector = PathBinaryDetector::new();
        assert!(detector.is_available(path).await);
        // Second call hits the memo; the answer is stable.
        assert!(detector.is_available(path).await);
        assert!(!detector.is_available("/no/such/bin").await);
    }
}
