//! [`SessionSettingsFile`]: a file written next to the session settings.

/// A file Delta writes next to the session settings file whenever it writes
/// that file, owner-only like it.
///
/// The settings file is handed to `claude --settings`, and its command
/// entries (the `SessionStart` hook and the `statusLine`) run `curl`. A
/// command line is visible to every local process, so whatever those
/// commands must not show — the hook secret in the URL they call — lives in
/// a file like this one, which the command reads (`curl -K <file>`). The
/// caller renders both the settings and these files; the use case only keeps
/// them on disk together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSettingsFile {
    /// Where the file is written.
    pub path: String,
    /// Its whole contents.
    pub contents: String,
}
