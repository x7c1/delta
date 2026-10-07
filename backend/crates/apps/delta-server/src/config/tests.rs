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
fn defaults_are_named_by_the_identifier_and_home_based() {
    let config = config(&[("HOME", "/home/u")]);
    assert_eq!(config.identifier, DEFAULT_IDENTIFIER);
    assert_eq!(config.worktree_base, "/home/u/.delta/worktrees");
    assert_eq!(config.transcript_root, "/home/u/.claude/projects");
    assert_eq!(config.port, 7878);
    let defaults = delta_usecase::LaunchConfig::default();
    assert_eq!(config.launch.claude_bin, defaults.claude_bin);
    assert_eq!(config.launch.echo_deadline, defaults.echo_deadline);
    assert_eq!(
        config.launch.slash_command_echo_deadline,
        defaults.slash_command_echo_deadline
    );
}

#[test]
fn the_default_data_dir_and_tmux_socket_are_named_by_the_identifier() {
    for (vars, identifier) in [
        (&[][..], DEFAULT_IDENTIFIER),
        (
            &[("DELTA_IDENTIFIER", "io.example.other")][..],
            "io.example.other",
        ),
    ] {
        let config = config(vars);
        assert_eq!(config.identifier, identifier);
        let data_dir = std::path::Path::new(&config.data_dir);
        assert!(
            data_dir.is_absolute(),
            "{} must not depend on the working directory",
            config.data_dir
        );
        assert!(
            data_dir.ends_with(identifier),
            "{} must end with the identifier",
            config.data_dir
        );
        assert_eq!(config.tmux_socket, identifier);
    }
}

#[test]
fn the_default_data_dir_is_the_platform_data_dir() {
    // The directory Tauri's `app_data_dir()` names for the desktop app, so an
    // existing install keeps its database.
    let config = config(&[]);
    assert_eq!(
        std::path::PathBuf::from(&config.data_dir),
        dirs::data_dir().unwrap().join(DEFAULT_IDENTIFIER)
    );
}

#[test]
fn a_shell_identifier_wins_over_the_variable() {
    let vars: HashMap<&str, OsString> = [("DELTA_IDENTIFIER", OsString::from("io.example.env"))]
        .into_iter()
        .collect();
    let config = desktop_config_from_vars(|name| vars.get(name).cloned(), "io.example.app");
    assert_eq!(config.identifier, "io.example.app");
    assert_eq!(config.tmux_socket, "io.example.app");
    assert!(config.data_dir.ends_with("io.example.app"));
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
        ("DELTA_IDENTIFIER", "io.example.delta"),
        ("DELTA_DATA_DIR", "/data/d"),
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
        ("DELTA_SLASH_COMMAND_ECHO_DEADLINE_MS", "50"),
    ]);
    assert_eq!(config.identifier, "io.example.delta");
    assert_eq!(config.data_dir, "/data/d");
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
    assert_eq!(
        config.launch.slash_command_echo_deadline,
        Duration::from_millis(50)
    );
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
        ("DELTA_IDENTIFIER", ""),
        ("DELTA_DATA_DIR", ""),
        ("DELTA_TMUX_SOCKET", ""),
    ]);
    assert_eq!(config.identifier, DEFAULT_IDENTIFIER);
    assert!(config.data_dir.ends_with(DEFAULT_IDENTIFIER));
    assert_eq!(config.tmux_socket, DEFAULT_IDENTIFIER);
    assert_eq!(config.transcript_root, "/home/u/.claude/projects");
    assert_eq!(
        config.launch.claude_bin,
        delta_usecase::LaunchConfig::default().claude_bin
    );
}

#[test]
fn the_release_feed_defaults_to_github_and_an_empty_value_turns_it_off() {
    assert_eq!(
        config(&[]).release_feed_url.as_deref(),
        Some("https://api.github.com/repos/x7c1/delta/releases/latest")
    );
    assert_eq!(
        config(&[("DELTA_RELEASE_FEED_URL", "http://127.0.0.1:9/latest")])
            .release_feed_url
            .as_deref(),
        Some("http://127.0.0.1:9/latest")
    );
    assert_eq!(
        config(&[("DELTA_RELEASE_FEED_URL", "")]).release_feed_url,
        None
    );
}

#[test]
fn an_unparseable_port_falls_back_to_the_default() {
    assert_eq!(config(&[("DELTA_PORT", "nope")]).port, DEFAULT_PORT);
}

#[test]
fn adopting_the_persisted_secret_replaces_the_minted_one_and_flags_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().to_string_lossy().into_owned();
    let start = || {
        let mut config = config(&[("DELTA_DATA_DIR", &data_dir)]);
        adopt_hook_secret(&mut config, None).unwrap();
        config
    };

    let first = start();
    assert!(first.hook_endpoint_changed, "a first run mints its secret");

    let second = start();
    assert_eq!(second.hook_secret, first.hook_secret);
    assert!(!second.hook_endpoint_changed);
    assert_eq!(
        second.session_settings_json(),
        first.session_settings_json()
    );
    assert_eq!(
        second.session_settings_path(),
        first.session_settings_path()
    );

    let mut overridden = config(&[("DELTA_DATA_DIR", &data_dir)]);
    adopt_hook_secret(&mut overridden, Some("hs".into())).unwrap();
    assert_eq!(overridden.hook_secret, "hs");
    assert!(overridden.hook_endpoint_changed);
}

#[test]
fn only_the_default_worktree_base_names_its_parent_for_erasing() {
    let home = || Some(OsString::from("/home/u"));
    assert_eq!(
        default_base_parent("/home/u/.delta/worktrees", home()).as_deref(),
        Some("/home/u/.delta")
    );
    assert_eq!(default_base_parent("/srv/worktrees", home()), None);
}

#[test]
fn only_the_desktop_shells_configuration_names_the_desktop_launcher() {
    assert_eq!(config(&[]).launcher, Launcher::Cli);
    assert_eq!(
        desktop_config_from_vars(|_| None, DEFAULT_IDENTIFIER).launcher,
        Launcher::Desktop
    );
}

#[test]
fn the_build_origin_is_the_one_this_build_was_compiled_with() {
    let compiled = BuildOrigin::from_build_env(option_env!("DELTA_BUILD_ORIGIN"));
    assert_eq!(build_origin(), compiled);
    assert_eq!(config(&[]).build_origin, compiled);
    // A run-time variable claims nothing.
    assert_eq!(
        config(&[("DELTA_BUILD_ORIGIN", "release")]).build_origin,
        compiled
    );
}
