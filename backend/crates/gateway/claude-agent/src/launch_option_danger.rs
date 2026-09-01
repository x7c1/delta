//! Which Claude launch options switch Claude Code's own permission system off.
//!
//! Its own module, beside the declared catalog, for the same reason the catalog
//! is here: the adapter that owns Claude owns Claude's vocabulary. The
//! composition root reads this through one per-provider accessor and hands it to
//! the domain as a [`LaunchOptionDangerPolicy`], which is what makes the registry
//! refuse to default-enable such an option and the browser mark it.
//!
//! Deliberately a **closed, short** list of spellings that mean "stop asking":
//! everything else stays the pass-through a launch option normally is. A flag
//! that merely widens what is auto-approved (`--permission-mode acceptEdits`)
//! is not on it — the permission system is still running and still refuses the
//! rest.
//!
//! [`LaunchOptionDangerPolicy`]: delta_usecase::LaunchOptionDangerPolicy

/// The `claude` flag that turns the permission system off wholesale.
///
/// Dangerous whatever `value` accompanies it: it is a valueless flag, so any
/// value is the user having typed something extra into the registry's optional
/// field, and none of those spellings makes the flag benign.
const SKIP_PERMISSIONS_FLAG: &str = "--dangerously-skip-permissions";
/// The `claude` flag selecting a permission mode. Benign for every mode but one.
const PERMISSION_MODE_FLAG: &str = "--permission-mode";
/// The one [`PERMISSION_MODE_FLAG`] value that is [`SKIP_PERMISSIONS_FLAG`] by
/// another name.
const BYPASS_PERMISSIONS_MODE: &str = "bypassPermissions";

/// Whether a Claude launch option `(name, value)` disables Claude Code's own
/// permission system.
///
/// `name` is a CLI flag and `value` its argument, exactly as the registry stores
/// them (`value` `None` for a valueless flag).
pub fn is_dangerous_launch_option(name: &str, value: Option<&str>) -> bool {
    match name {
        SKIP_PERMISSIONS_FLAG => true,
        PERMISSION_MODE_FLAG => value == Some(BYPASS_PERMISSIONS_MODE),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_permissions_is_dangerous_with_or_without_a_value() {
        assert!(is_dangerous_launch_option(SKIP_PERMISSIONS_FLAG, None));
        assert!(is_dangerous_launch_option(
            SKIP_PERMISSIONS_FLAG,
            Some("true")
        ));
    }

    #[test]
    fn permission_mode_is_dangerous_only_for_bypass() {
        assert!(is_dangerous_launch_option(
            PERMISSION_MODE_FLAG,
            Some(BYPASS_PERMISSIONS_MODE)
        ));
        // The rest of the mode enum as `claude --help` states it, so the
        // "only for bypass" claim is checked against the real neighbours
        // rather than made-up ones.
        for benign in ["default", "acceptEdits", "plan"] {
            assert!(
                !is_dangerous_launch_option(PERMISSION_MODE_FLAG, Some(benign)),
                "`{PERMISSION_MODE_FLAG} {benign}` leaves the permission system running"
            );
        }
        // No value at all is not the bypass mode either — the CLI would reject
        // the flag, which is the agent's business, not a safety bypass.
        assert!(!is_dangerous_launch_option(PERMISSION_MODE_FLAG, None));
    }

    #[test]
    fn an_ordinary_flag_is_not_dangerous() {
        assert!(!is_dangerous_launch_option("--model", Some("opus")));
        assert!(!is_dangerous_launch_option("--plugin-dir", Some("/opt/p")));
    }
}
