//! What a worktree launch asks the user before Claude Code's trust is
//! pre-accepted for it, and which paths count as agent configuration.
//!
//! Workspace trust is what lets a project's checked-in agent configuration —
//! hooks, status line, `apiKeyHelper`, MCP servers, `CLAUDE.md` imports — take
//! effect without a prompt. Delta pre-accepts it for a worktree it builds only
//! when the user has, in effect, already made that decision: the repository
//! the worktree came from is trusted, and the worktree's agent configuration
//! is the same as the repository `HEAD` the user trusted. Anything else is a
//! [`LaunchTrustCheck`] that needs confirmation. The rule is described in full
//! in `docs/guides/security.md`.

/// The most changed paths a check renders a diff for. Every changed path is
/// still listed; past this many, a path is listed with its diff omitted
/// ([`AgentConfigChange::diff_truncated`]), so one branch cannot make the
/// check run an unbounded number of `git diff`s.
pub const MAX_DIFFED_PATHS: usize = 50;

/// The longest diff text a check returns for one path, in bytes. A longer
/// diff is cut (at a character boundary) and marked truncated.
pub const MAX_DIFF_BYTES: usize = 64 * 1024;

/// One agent-configuration path whose content differs between the
/// repository `HEAD` the user trusts and the commit a worktree launches on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfigChange {
    /// The path, relative to the repository top.
    pub path: String,
    /// The unified diff of the path between the two commits, as git renders
    /// it. Empty when the diff was omitted (see `diff_truncated`).
    pub diff: String,
    /// Whether `diff` is incomplete: cut at [`MAX_DIFF_BYTES`], or omitted for
    /// a path past [`MAX_DIFFED_PATHS`].
    pub diff_truncated: bool,
}

/// The answer to "may Delta pre-accept Claude Code's trust for this worktree
/// launch without asking?".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchTrustCheck {
    /// The repository the worktree comes from is not trusted in Claude Code's
    /// config, so launching would be the first time its configuration is
    /// trusted.
    pub repository_untrusted: bool,
    /// The commit the worktree will be on — what the user confirms, and what
    /// the launch request carries back as `confirmed_commit`. `None` when the
    /// launch never pre-accepts trust at all (nothing to confirm).
    pub commit: Option<String>,
    /// The agent-configuration paths whose content differs from the trusted
    /// `HEAD`, for a worktree started from a remote branch. Always empty for a
    /// worktree started from the user's own `HEAD`.
    pub agent_config_changes: Vec<AgentConfigChange>,
}

impl LaunchTrustCheck {
    /// A launch that needs no confirmation because Delta will not pre-accept
    /// trust for it (Claude Code's own dialog, if any, applies).
    pub fn not_applicable() -> Self {
        Self::default()
    }

    /// Whether the launch must be confirmed before trust is pre-accepted.
    pub fn needs_confirmation(&self) -> bool {
        self.repository_untrusted || !self.agent_config_changes.is_empty()
    }
}

/// Whether `path` (relative to the repository top, `/`-separated) is agent
/// configuration a worktree launch compares: anything under the top-level
/// `.claude/` directory, the top-level `.mcp.json`, and every `CLAUDE.md` or
/// `CLAUDE.local.md` at any depth.
///
/// `.claude/` is matched as a whole directory rather than as a list of known
/// files, so a configuration file Claude Code starts reading later is covered
/// without a change here. Names are compared case-insensitively: on a
/// case-insensitive filesystem (the macOS default) a differently-cased name is
/// the same file to Claude Code.
pub fn is_agent_config_path(path: &str) -> bool {
    let mut components = path.split('/').filter(|c| !c.is_empty());
    let Some(first) = components.next() else {
        return false;
    };
    if first.eq_ignore_ascii_case(".claude") {
        return true;
    }
    let last = path.rsplit('/').find(|c| !c.is_empty()).unwrap_or(first);
    let is_top_level = components.next().is_none();
    (is_top_level && first.eq_ignore_ascii_case(".mcp.json"))
        || last.eq_ignore_ascii_case("CLAUDE.md")
        || last.eq_ignore_ascii_case("CLAUDE.local.md")
}

/// `diff` cut to at most [`MAX_DIFF_BYTES`] at a character boundary, and
/// whether anything was cut.
pub(crate) fn truncate_diff(mut diff: String) -> (String, bool) {
    if diff.len() <= MAX_DIFF_BYTES {
        return (diff, false);
    }
    let mut end = MAX_DIFF_BYTES;
    while !diff.is_char_boundary(end) {
        end -= 1;
    }
    diff.truncate(end);
    (diff, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_claude_directory_is_compared_whole() {
        for path in [
            ".claude/settings.json",
            ".claude/settings.local.json",
            ".claude/a-file-added-later.toml",
            ".claude/agents/reviewer.md",
            ".Claude/settings.json",
        ] {
            assert!(is_agent_config_path(path), "{path} is agent configuration");
        }
    }

    #[test]
    fn top_level_mcp_json_and_claude_md_at_any_depth_are_compared() {
        for path in [
            ".mcp.json",
            ".MCP.json",
            "CLAUDE.md",
            "CLAUDE.local.md",
            "docs/CLAUDE.md",
            "a/b/c/claude.md",
            "pkg/CLAUDE.local.md",
        ] {
            assert!(is_agent_config_path(path), "{path} is agent configuration");
        }
    }

    #[test]
    fn other_paths_are_not_compared() {
        for path in [
            "src/main.rs",
            "README.md",
            "docs/.claude/settings.json",
            "pkg/.mcp.json",
            "CLAUDE.md.bak",
            "NOTCLAUDE.md",
            ".claudeignore",
            "",
        ] {
            assert!(
                !is_agent_config_path(path),
                "{path:?} is not agent configuration"
            );
        }
    }

    #[test]
    fn needs_confirmation_when_untrusted_or_changed() {
        assert!(!LaunchTrustCheck::not_applicable().needs_confirmation());
        assert!(LaunchTrustCheck {
            repository_untrusted: true,
            ..LaunchTrustCheck::default()
        }
        .needs_confirmation());
        assert!(LaunchTrustCheck {
            agent_config_changes: vec![AgentConfigChange {
                path: ".mcp.json".into(),
                diff: String::new(),
                diff_truncated: false,
            }],
            ..LaunchTrustCheck::default()
        }
        .needs_confirmation());
    }

    #[test]
    fn truncate_diff_cuts_at_a_char_boundary() {
        let short = "@@\n-a\n+b\n".to_owned();
        assert_eq!(truncate_diff(short.clone()), (short, false));

        let long = "é".repeat(MAX_DIFF_BYTES);
        let (cut, truncated) = truncate_diff(long);
        assert!(truncated);
        assert!(cut.len() <= MAX_DIFF_BYTES);
        assert!(cut.chars().all(|c| c == 'é'));
    }
}
