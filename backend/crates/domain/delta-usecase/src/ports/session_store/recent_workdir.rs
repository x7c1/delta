//! One recently-used working directory.

/// One recently-used working directory: its absolute `cwd` and the timestamp of
/// the latest activity in any session that used it (`MAX(message.created_at)`,
/// falling back to the session's `created_at`; `None` only when a contributing
/// session is itself message-less and has a null `created_at`, which does not
/// occur in practice). Derived from the `session.cwd` column — Delta keeps no
/// separate working-directory history.
pub type RecentWorkdir = (String, Option<String>);
