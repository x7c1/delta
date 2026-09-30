use super::*;

use std::collections::HashMap;
use std::time::Duration;

fn config(vars: &[(&str, &str)]) -> Config {
    let vars: HashMap<String, OsString> = vars
        .iter()
        .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
        .collect();
    config_from_vars(|name| vars.get(name).cloned())
}

#[test]
fn defaults_are_cwd_relative_and_home_based() {
    let config = config(&[("HOME", "/home/u")]);
    assert_eq!(config.database_path, "delta.db");
    assert_eq!(config.session_workdir_base, ".tmp/session");
    assert_eq!(config.worktree_base, "/home/u/.delta/worktrees");
    assert_eq!(config.transcript_root, "/home/u/.claude/projects");
    assert_eq!(config.tmux_socket, delta_bootstrap::DEFAULT_TMUX_SOCKET);
    assert_eq!(config.port, 7878);
    let defaults = delta_usecase::LaunchConfig::default();
    assert_eq!(config.launch.claude_bin, defaults.claude_bin);
    assert_eq!(config.launch.echo_deadline, defaults.echo_deadline);
}

#[test]
fn unset_home_falls_back_to_the_temp_dir() {
    let config = config(&[]);
    let temp = std::env::temp_dir();
    assert_eq!(
        config.worktree_base,
        temp.join(".delta/worktrees").to_string_lossy()
    );
    assert_eq!(
        config.transcript_root,
        temp.join(".claude/projects").to_string_lossy()
    );
}

#[test]
fn minted_secrets_are_64_hex_characters_and_distinct() {
    let config = config(&[("DELTA_AUTH_TOKEN", ""), ("DELTA_HOOK_SECRET", "")]);
    for secret in [&config.auth_token, &config.hook_secret] {
        assert_eq!(secret.len(), 64);
        assert!(secret.chars().all(|c| c.is_ascii_hexdigit()));
    }
    assert_ne!(config.auth_token, config.hook_secret);
}

#[test]
fn explicit_variables_override_every_default() {
    let config = config(&[
        ("HOME", "/home/u"),
        ("DELTA_DB_PATH", "/data/d.db"),
        ("DELTA_SESSION_WORKDIR", "/data/s"),
        ("DELTA_WORKTREE_BASE", "/data/w"),
        ("DELTA_TMUX_SOCKET", "sock"),
        ("DELTA_AUTH_TOKEN", "tok"),
        ("DELTA_HOOK_SECRET", "hs"),
        ("DELTA_TRANSCRIPT_ROOT", "/data/t"),
        ("DELTA_PORT", "9000"),
        ("DELTA_CLAUDE_BIN", "/bin/fake-claude"),
        ("DELTA_LAUNCH_DEADLINE_MS", "10"),
        ("DELTA_LAUNCH_PREP_DEADLINE_MS", "20"),
        ("DELTA_PERMISSION_DECISION_TIMEOUT_MS", "30"),
        ("DELTA_ECHO_DEADLINE_MS", "40"),
    ]);
    assert_eq!(config.database_path, "/data/d.db");
    assert_eq!(config.session_workdir_base, "/data/s");
    assert_eq!(config.worktree_base, "/data/w");
    assert_eq!(config.tmux_socket, "sock");
    assert_eq!(config.auth_token, "tok");
    assert_eq!(config.hook_secret, "hs");
    assert_eq!(config.transcript_root, "/data/t");
    assert_eq!(config.port, 9000);
    assert_eq!(config.launch.claude_bin, "/bin/fake-claude");
    assert_eq!(
        config.launch.pending_spawn_deadline,
        Duration::from_millis(10)
    );
    assert_eq!(
        config.launch.resume_ready_deadline,
        Duration::from_millis(10)
    );
    assert_eq!(
        config.launch.launch_prep_deadline,
        Duration::from_millis(20)
    );
    assert_eq!(
        config.launch.permission_decision_deadline,
        Duration::from_millis(30)
    );
    assert_eq!(config.launch.echo_deadline, Duration::from_millis(40));
}

#[test]
fn claude_config_dir_moves_the_transcript_root() {
    let config = config(&[("HOME", "/home/u"), ("CLAUDE_CONFIG_DIR", "/cfg")]);
    assert_eq!(config.transcript_root, "/cfg/projects");
}

#[test]
fn empty_overrides_that_must_not_be_empty_fall_back() {
    let config = config(&[
        ("HOME", "/home/u"),
        ("DELTA_TRANSCRIPT_ROOT", ""),
        ("CLAUDE_CONFIG_DIR", ""),
        ("DELTA_CLAUDE_BIN", ""),
    ]);
    assert_eq!(config.transcript_root, "/home/u/.claude/projects");
    assert_eq!(
        config.launch.claude_bin,
        delta_usecase::LaunchConfig::default().claude_bin
    );
}

#[test]
fn an_unparseable_port_falls_back_to_the_default() {
    assert_eq!(config(&[("DELTA_PORT", "nope")]).port, DEFAULT_PORT);
}
