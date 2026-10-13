//! The `git` command every operation starts from.

use super::*;

#[test]
fn every_git_command_carries_the_given_environment() {
    let git = Git::new(vec![("PATH".to_owned(), "/opt/homebrew/bin".to_owned())]);

    let command = git.git("/repo");

    let envs: Vec<_> = command.as_std().get_envs().collect();
    assert_eq!(
        envs,
        [("PATH".as_ref(), Some("/opt/homebrew/bin".as_ref()))]
    );
    let args: Vec<_> = command.as_std().get_args().collect();
    assert_eq!(args, ["-C", "/repo"]);
}
