//! What Delta ships for Claude: the declared launch-option catalog and the
//! guard that keeps it shippable.
//!
//! Its own module because the catalog grows an entry at a time, while the rest
//! of this crate is about driving a live `claude` session.

use delta_usecase::{AgentProvider, LaunchOptionPreset};

/// The launch options Delta ships for Claude.
///
/// Declared in the adapter that owns Claude, beside [`CLAUDE_CAPABILITIES`] —
/// whose `launch_option_style` is what says a Claude option's `name` is a CLI
/// flag — for the same reason the capability profile lives there: the adapter
/// that owns the behaviour owns the vocabulary. The composition root reads this
/// through one per-provider accessor and materializes every entry into the
/// launch-option registry at startup, so each is an ordinary registry row the
/// user can tick.
///
/// Deliberately short: these are the flags in daily use, not an inventory of
/// everything `claude` accepts. Anything else is the user's own row.
///
/// `--model` ships the documented **aliases** (`opus`, `fable`, `sonnet`), which
/// track the latest model of each family, so an entry cannot go stale; a
/// concrete slug such as `claude-fable-5` would be a dated snapshot and is not
/// shipped. `--permission-mode` ships only `auto`; the CLI also accepts
/// `acceptEdits`, `bypassPermissions`, `manual`, `dontAsk` and `plan`, but
/// listing values nobody selects would only make the picker longer.
///
/// The three `--model` entries are **mutually exclusive**, and nothing enforces
/// that: the picker is a plain multi-select, so a user can tick two, and this
/// adapter's `launch` then puts `--model` into argv twice and leaves the outcome
/// to the CLI. Shipping one row per alias is still the right
/// shape — a single row could only name one model — but it is worth knowing that
/// the exclusivity is the user's to respect, the same way it already is for two
/// hand-registered rows naming one flag.
///
/// No entry may name a flag Delta sets itself — `--settings`, `--session-id`,
/// `--resume`, the consts the parent module declares beside the launch path. The
/// guard test below pins that, because unlike Codex the Claude launch has no
/// rejection path at all: such an entry would ride into argv beside Delta's own
/// copy and break every session started with it, silently.
///
/// [`CLAUDE_CAPABILITIES`]: super::CLAUDE_CAPABILITIES
pub const CLAUDE_LAUNCH_OPTION_CATALOG: &[LaunchOptionPreset] = &[
    LaunchOptionPreset {
        key: "claude:model-opus",
        label: "Opus",
        name: "--model",
        value: Some("opus"),
        provider: AgentProvider::Claude,
    },
    LaunchOptionPreset {
        key: "claude:model-fable",
        label: "Fable",
        name: "--model",
        value: Some("fable"),
        provider: AgentProvider::Claude,
    },
    LaunchOptionPreset {
        key: "claude:model-sonnet",
        label: "Sonnet",
        name: "--model",
        value: Some("sonnet"),
        provider: AgentProvider::Claude,
    },
    LaunchOptionPreset {
        key: "claude:permission-mode-auto",
        label: "Permission mode: auto",
        name: "--permission-mode",
        value: Some("auto"),
        provider: AgentProvider::Claude,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{RESUME_FLAG, SESSION_ID_FLAG, SETTINGS_FLAG};

    /// No shipped option may name a flag Delta fills in itself.
    ///
    /// This is the guard the declared catalog exists for. Claude's launch drops
    /// a launch option's `name` straight into argv with **no** rejection path
    /// (unlike Codex, which refuses a Delta-owned field), so a catalog entry
    /// naming `--session-id` or `--settings` would appear twice on the command
    /// line and break every session started with it — silently, from the user's
    /// point of view.
    #[test]
    fn no_shipped_option_names_a_delta_owned_flag() {
        for preset in CLAUDE_LAUNCH_OPTION_CATALOG {
            for owned in [SETTINGS_FLAG, SESSION_ID_FLAG, RESUME_FLAG] {
                assert_ne!(
                    preset.name, owned,
                    "built-in `{}` names `{owned}`, which Delta sets itself",
                    preset.key
                );
            }
        }
    }
}
