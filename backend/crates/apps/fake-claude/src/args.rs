//! The slice of `claude`'s CLI surface that Delta uses.
//!
//! Delta launches `claude --settings <file> --session-id <uuid> [prompt]` for
//! a fresh spawn and `claude --settings <file> --resume <id>` for a resume.
//! Anything else on the command line is tolerated and ignored, mirroring how
//! a newer Delta could pass a flag this fake does not know about without
//! breaking the harness.

/// The launch arguments the fake acts on.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// `--session-id <uuid>`: the Delta-minted id pinning a fresh conversation.
    pub session_id: Option<String>,
    /// `--settings <file>`: the settings JSON carrying the hook URLs.
    pub settings: Option<String>,
    /// `--resume <id>`: reattach to the stored conversation with this id.
    pub resume: Option<String>,
    /// The trailing positional argument: a first prompt that `claude`
    /// auto-submits at startup.
    pub prompt: Option<String>,
}

impl Args {
    /// Parse the process arguments (without argv[0]).
    ///
    /// Unknown `--flags` are skipped; a non-flag argument is the positional
    /// prompt (the last one wins, like a CLI that takes a single positional).
    pub fn parse(argv: impl IntoIterator<Item = String>) -> Self {
        let mut args = Self::default();
        let mut iter = argv.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--session-id" => args.session_id = iter.next(),
                "--settings" => args.settings = iter.next(),
                "--resume" => args.resume = iter.next(),
                other if other.starts_with('-') => {
                    // An unknown flag. Its value (if any) cannot be told apart
                    // from a positional prompt without knowing the flag, so only
                    // the flag itself is skipped — Delta never passes unknown
                    // value-taking flags, so this stays unambiguous in practice.
                }
                _ => args.prompt = Some(arg),
            }
        }
        args
    }

    /// The conversation's session id: pinned for a fresh spawn, recalled for a
    /// resume.
    pub fn effective_session_id(&self) -> Option<&str> {
        self.resume.as_deref().or(self.session_id.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Args {
        Args::parse(argv.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn parses_the_fresh_spawn_command_line() {
        let args = parse(&[
            "--settings",
            "/run/delta/settings.json",
            "--session-id",
            "0190-uuid",
            "hello world",
        ]);
        assert_eq!(args.settings.as_deref(), Some("/run/delta/settings.json"));
        assert_eq!(args.session_id.as_deref(), Some("0190-uuid"));
        assert_eq!(args.prompt.as_deref(), Some("hello world"));
        assert_eq!(args.effective_session_id(), Some("0190-uuid"));
    }

    #[test]
    fn parses_the_resume_command_line() {
        let args = parse(&["--settings", "/s.json", "--resume", "abc"]);
        assert_eq!(args.resume.as_deref(), Some("abc"));
        assert_eq!(args.prompt, None);
        assert_eq!(args.effective_session_id(), Some("abc"));
    }

    #[test]
    fn unknown_flags_are_skipped() {
        let args = parse(&["--verbose", "--session-id", "id", "prompt"]);
        assert_eq!(args.session_id.as_deref(), Some("id"));
        assert_eq!(args.prompt.as_deref(), Some("prompt"));
    }
}
