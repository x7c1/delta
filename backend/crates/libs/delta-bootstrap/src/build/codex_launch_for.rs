use codex_agent::CodexLaunchConfig;

use crate::Config;

/// The Codex launch configuration for `config`.
///
/// The app-server is spawned with [`Config::child_env`] set. `DELTA_CODEX_BIN`
/// substitutes the `codex` command it is spawned from (default the bare
/// `codex`, resolved via `PATH`), mirroring `DELTA_CLAUDE_BIN` for the Claude
/// launch. Only the binary is configurable in this slice; the default
/// `app-server` argument is kept.
///
/// The variable is read here in the composition root — rather than threaded
/// through [`Config`] — so every existing `Config` construction stays
/// untouched. Reading it has no side effect: the resulting config is only
/// stored on the factory and no process is spawned until a Codex session needs
/// one.
pub(super) fn codex_launch_for(config: &Config) -> CodexLaunchConfig {
    let mut codex = CodexLaunchConfig {
        env: config.child_env.clone(),
        ..CodexLaunchConfig::default()
    };
    if let Ok(bin) = std::env::var("DELTA_CODEX_BIN") {
        if !bin.is_empty() {
            codex.codex_bin = bin;
        }
    }
    codex
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::build::testing::test_config;

    #[test]
    fn the_codex_app_server_is_launched_with_the_child_env() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config(&dir);
        config.child_env = vec![("PATH".into(), "/opt/homebrew/bin".into())];

        assert_eq!(codex_launch_for(&config).env, config.child_env);
    }
}
