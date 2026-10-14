//! `DELTA_AUTH_TOKEN` fixes the bearer token in advance, but nothing the server
//! starts may inherit it: every agent, and every command an agent runs, would
//! otherwise be able to read the token out of its environment.
//!
//! Its own test binary, because it sets and reads the process environment,
//! which no other test in the same process may race.

use std::process::Command;

#[test]
fn the_token_is_read_and_withheld_from_the_commands_the_server_starts() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("DELTA_DATA_DIR", dir.path());
    std::env::set_var("DELTA_AUTH_TOKEN", "fixed-in-advance");

    let config = delta_server::config::config_from_env().unwrap();

    assert_eq!(config.auth_token, "fixed-in-advance");
    assert!(
        std::env::var_os("DELTA_AUTH_TOKEN").is_none(),
        "the server's own environment no longer holds the token"
    );
    assert!(
        config
            .child_env
            .iter()
            .all(|(name, _)| name != "DELTA_AUTH_TOKEN"),
        "the environment set on top of the inherited one does not carry it"
    );

    // What every command the server starts — tmux, and through it each agent —
    // inherits: the process environment with the configuration's `child_env`
    // on top, as the gateways spawn them.
    let output = Command::new("env")
        .envs(config.child_env.iter().map(|(name, value)| (name, value)))
        .output()
        .unwrap();
    let env = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        !env.contains("DELTA_AUTH_TOKEN") && !env.contains("fixed-in-advance"),
        "a spawned command sees no token: {env}"
    );
}
