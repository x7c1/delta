use std::ffi::OsString;

/// Why the login shell's `PATH` was not adopted, kept until startup finishes.
///
/// The import only warns and carries on with the inherited `PATH`, which may
/// still hold every command Delta needs (a launch from a terminal, say). When
/// it does not, startup stops at the first missing command, and that error
/// alone ("required command 'tmux' was not found on PATH") reads as if the
/// command were not installed. This record lets the dialog say that the login
/// shell's `PATH` was never read, why, and where the command was looked for
/// instead.
#[derive(Debug)]
pub struct PathNotImported {
    /// The login shell that was asked.
    pub shell: OsString,
    /// Why its `PATH` could not be adopted.
    pub reason: String,
    /// The `PATH` the app was started with, which the commands are looked up
    /// in instead.
    pub inherited_path: Option<OsString>,
}

impl PathNotImported {
    /// Append the explanation to a startup error `message` when `err` is a
    /// missing command; return `message` unchanged for any other error, which
    /// the `PATH` has nothing to do with.
    pub fn explain(&self, message: String, err: &anyhow::Error) -> String {
        let missing_command = matches!(
            err.downcast_ref::<delta_bootstrap::Error>(),
            Some(delta_bootstrap::Error::MissingCommand { .. })
        );
        if !missing_command {
            return message;
        }
        let inherited_path = self
            .inherited_path
            .as_ref()
            .map_or_else(|| "(unset)".into(), |path| path.to_string_lossy());
        format!(
            "{message}\n\nDelta could not read PATH from your login shell ({}): {}. \
             It looked only in the PATH it was started with: {inherited_path}",
            self.shell.to_string_lossy(),
            self.reason,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn not_imported() -> PathNotImported {
        PathNotImported {
            shell: "/bin/zsh".into(),
            reason: "no answer within 8s".to_owned(),
            inherited_path: Some("/usr/bin:/bin".into()),
        }
    }

    #[test]
    fn a_missing_command_is_explained_by_the_unread_path() {
        let err = anyhow::Error::new(delta_bootstrap::Error::MissingCommand {
            bin: "tmux".to_owned(),
        });

        let message = not_imported().explain(err.to_string(), &err);

        assert_eq!(
            message,
            "required command 'tmux' was not found on PATH\n\n\
             Delta could not read PATH from your login shell (/bin/zsh): no answer within 8s. \
             It looked only in the PATH it was started with: /usr/bin:/bin"
        );
    }

    #[test]
    fn other_startup_errors_are_left_alone() {
        let err = anyhow::anyhow!("the hook state file is unreadable");

        let message = not_imported().explain("unchanged".to_owned(), &err);

        assert_eq!(message, "unchanged");
    }
}
