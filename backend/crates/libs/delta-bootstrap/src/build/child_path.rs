use std::ffi::OsString;

use crate::Config;

/// The `PATH` bare command names are resolved on: the one in
/// [`Config::child_env`] when it carries one, since that is what the spawns
/// run with, else the process's own.
pub(super) fn child_path(config: &Config) -> Option<OsString> {
    config
        .child_env
        .iter()
        .rev()
        .find(|(name, _)| name == "PATH")
        .map(|(_, value)| OsString::from(value))
        .or_else(|| std::env::var_os("PATH"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::build::testing::test_config;

    #[test]
    fn bare_names_are_resolved_on_the_child_path_when_there_is_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config(&dir);
        config.child_env = vec![
            ("LANG".into(), "en_US.UTF-8".into()),
            ("PATH".into(), "/opt/homebrew/bin:/usr/bin".into()),
        ];

        assert_eq!(
            child_path(&config),
            Some(OsString::from("/opt/homebrew/bin:/usr/bin"))
        );
    }

    #[test]
    fn bare_names_fall_back_to_the_process_path() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(&dir);

        assert_eq!(child_path(&config), std::env::var_os("PATH"));
    }
}
