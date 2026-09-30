//! Launch overrides read from `DELTA_*` variables.

/// Launch overrides from the environment, defaulting to production values.
///
/// - `DELTA_CLAUDE_BIN` substitutes the binary launched in each tmux session
///   (default `claude`). Lets tests and alternative installs supply a stand-in
///   or an out-of-`PATH` binary; the spawn command line is otherwise identical.
/// - `DELTA_LAUNCH_DEADLINE_MS` shrinks (or stretches) the launch watchdog —
///   both the unbound-fresh-spawn deadline and the resume-readiness deadline,
///   which share the same production value — so a "launch never came up" path
///   can be exercised quickly under test.
/// - `DELTA_LAUNCH_PREP_DEADLINE_MS` shrinks (or stretches) how long an
///   accepted session's background launch preparation (worktree build, trust
///   seed, settings write, agent launch) may run before it is abandoned, so
///   the "preparation never finished" path can be exercised in milliseconds
///   instead of the production ten minutes.
/// - `DELTA_PERMISSION_DECISION_TIMEOUT_MS` shrinks (or stretches) how long
///   the `PermissionRequest` hook response waits for a browser decision
///   before falling back to the TUI prompt, so the passthrough path can be
///   exercised quickly under test.
/// - `DELTA_ECHO_DEADLINE_MS` shrinks (or stretches) how long a dispatched
///   send waits for its `UserPromptSubmit` echo before the watchdog gives up
///   on it, so the retry-then-park path for swallowed keystrokes can be
///   exercised in seconds instead of minutes.
/// - `DELTA_SLASH_COMMAND_ECHO_DEADLINE_MS` shrinks (or stretches) the much
///   shorter wait after which a silent slash-command send is taken to be a
///   local command that ran, so a test can free the session in milliseconds.
pub(super) fn launch_from_vars(
    text: &dyn Fn(&str) -> Option<String>,
) -> delta_usecase::LaunchConfig {
    let millis = |name: &str| {
        text(name)
            .and_then(|v| v.parse::<u64>().ok())
            .map(std::time::Duration::from_millis)
    };
    let mut launch = delta_usecase::LaunchConfig::default();
    if let Some(bin) = text("DELTA_CLAUDE_BIN").filter(|bin| !bin.is_empty()) {
        launch.claude_bin = bin;
    }
    if let Some(deadline) = millis("DELTA_LAUNCH_DEADLINE_MS") {
        launch.pending_spawn_deadline = deadline;
        launch.resume_ready_deadline = deadline;
    }
    if let Some(deadline) = millis("DELTA_LAUNCH_PREP_DEADLINE_MS") {
        launch.launch_prep_deadline = deadline;
    }
    if let Some(deadline) = millis("DELTA_PERMISSION_DECISION_TIMEOUT_MS") {
        launch.permission_decision_deadline = deadline;
    }
    if let Some(deadline) = millis("DELTA_ECHO_DEADLINE_MS") {
        launch.echo_deadline = deadline;
    }
    if let Some(deadline) = millis("DELTA_SLASH_COMMAND_ECHO_DEADLINE_MS") {
        launch.slash_command_echo_deadline = deadline;
    }
    launch
}
