use crate::{Config, DEFAULT_IDENTIFIER};

/// A configuration whose data directory is `dir`.
pub(crate) fn test_config(dir: &tempfile::TempDir) -> Config {
    Config {
        identifier: DEFAULT_IDENTIFIER.into(),
        data_dir: dir.path().to_string_lossy().into_owned(),
        worktree_base: "/tmp/delta-worktrees".into(),
        tmux_socket: DEFAULT_IDENTIFIER.into(),
        auth_token: "test-token".into(),
        hook_secret: "test-hook-secret".into(),
        transcript_root: "/tmp".into(),
        port: 7878,
        hook_endpoint_changed: false,
        launch: delta_usecase::LaunchConfig::default(),
        child_env: Vec::new(),
        release_feed_url: None,
    }
}
