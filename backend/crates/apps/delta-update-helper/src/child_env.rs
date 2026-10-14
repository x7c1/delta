//! Running one test again in a child process with environment variables
//! set, for the tests showing the helper ignores them.
//!
//! The variables are set on the child only: setting one in the test process
//! would race the other tests' threads, which read the environment too.

use std::ffi::OsStr;
use std::process::Command;

/// Set on the child, so the test it runs knows it is the child.
const CHILD: &str = "DELTA_UPDATE_HELPER_TEST_CHILD";

/// Run the test `name` (its full path, `module::tests::name`) of this test
/// binary in a child process with `vars` set, and fail unless it ran and
/// passed there.
///
/// The test runs there with `--include-ignored`: it is marked `#[ignore]`
/// so a plain `cargo test` runs it only through this, and it returns early
/// unless [`in_child`].
pub fn run_in_child(name: &str, vars: &[(&str, &OsStr)]) {
    let exe = std::env::current_exe().expect("the test binary's path");
    let output = Command::new(exe)
        .args([name, "--exact", "--include-ignored", "--test-threads=1"])
        .env(CHILD, "1")
        .envs(vars.iter().copied())
        .output()
        .expect("the test binary runs again");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed"),
        "{name} failed in its child process ({}):\n{stdout}\n{stderr}",
        output.status
    );
}

/// Whether this process is the child [`run_in_child`] started.
pub fn in_child() -> bool {
    std::env::var_os(CHILD).is_some()
}
