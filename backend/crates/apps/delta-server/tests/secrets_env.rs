//! `DELTA_AUTH_TOKEN` fixes the bearer token in advance and `DELTA_HOOK_SECRET`
//! the hook secret, but nothing the server starts may inherit either: every
//! agent, and every command an agent runs, would otherwise be able to read them
//! out of its environment — the token to call the API, the secret to post as
//! one of Delta's sessions.
//!
//! Its own test binary, because it sets and reads the process environment,
//! which no other test in the same process may race.

use std::process::Command;

const HOOK_SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn the_secrets_are_read_and_withheld_from_the_commands_the_server_starts() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("DELTA_DATA_DIR", dir.path());
    std::env::set_var("DELTA_AUTH_TOKEN", "fixed-in-advance");
    std::env::set_var("DELTA_HOOK_SECRET", HOOK_SECRET);

    let mut config = delta_server::config::config_from_env().unwrap();

    assert_eq!(config.auth_token, "fixed-in-advance");
    for name in ["DELTA_AUTH_TOKEN", "DELTA_HOOK_SECRET"] {
        assert!(
            std::env::var_os(name).is_none(),
            "the server's own environment no longer holds {name}"
        );
        assert!(
            config.child_env.iter().all(|(n, _)| n != name),
            "the environment set on top of the inherited one does not carry {name}"
        );
    }

    // The hook secret still wins over the persisted one although the variable
    // is gone by the time it is adopted.
    delta_server::config::adopt_persisted_hook_secret(&mut config).unwrap();
    assert_eq!(config.hook_secret, HOOK_SECRET);

    // What every command the server starts — tmux, and through it each agent —
    // inherits: the process environment with the configuration's `child_env`
    // on top, as the gateways spawn them.
    let output = Command::new("env")
        .envs(config.child_env.iter().map(|(name, value)| (name, value)))
        .output()
        .unwrap();
    let env = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    for leaked in [
        "DELTA_AUTH_TOKEN",
        "fixed-in-advance",
        "DELTA_HOOK_SECRET",
        HOOK_SECRET,
    ] {
        assert!(
            !env.contains(leaked),
            "a spawned command sees no {leaked}: {env}"
        );
    }
}
