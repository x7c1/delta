//! Whether the directory holding the app bundle is one only this user (or
//! root, or an administrator) may change.

use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Write permission for the group.
const GROUP_WRITE: u32 = 0o020;
/// Write permission for everyone else.
const OTHERS_WRITE: u32 = 0o002;
/// The sticky bit: only an entry's owner (or the directory's, or root) may
/// rename or remove it.
const STICKY: u32 = 0o1000;

/// The groups whose members may write where the bundle is without that
/// letting anyone new replace it: they can become root already. On macOS,
/// `wheel` (0) and `admin` (80), which own `/Applications` (`root:admin`,
/// mode 0775); elsewhere, root's group.
#[cfg(target_os = "macos")]
pub(crate) const ADMIN_GROUPS: &[u32] = &[0, 80];
#[cfg(not(target_os = "macos"))]
pub(crate) const ADMIN_GROUPS: &[u32] = &[0];

/// Why someone other than `user` could replace an entry of `dir`, the
/// directory holding the bundle, if anyone could: an update installed there
/// could be swapped for another app before it next starts. `None` when
/// `dir` is owned by `user` or root, and either no one but its owner and an
/// administrators' group may write to it, or its sticky bit keeps others
/// from renaming what they do not own.
pub(crate) fn shared_by_others(dir: &Path, user: u32) -> Result<Option<String>, std::io::Error> {
    let metadata = std::fs::metadata(dir)?;
    Ok(exposure(
        metadata.uid(),
        metadata.gid(),
        metadata.mode(),
        user,
    ))
}

/// [`shared_by_others`] for a directory owned by `owner` and group `group`,
/// with permission bits `mode`.
fn exposure(owner: u32, group: u32, mode: u32, user: u32) -> Option<String> {
    if owner != user && owner != 0 {
        return Some(format!("is owned by uid {owner}, neither you nor root"));
    }
    if mode & STICKY != 0 {
        return None;
    }
    if mode & OTHERS_WRITE != 0 {
        return Some(format!(
            "may be written to by every user (mode {:o})",
            mode & 0o7777
        ));
    }
    if mode & GROUP_WRITE != 0 && !ADMIN_GROUPS.contains(&group) {
        return Some(format!(
            "may be written to by its group, gid {group} (mode {:o})",
            mode & 0o7777
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: u32 = 501;
    const STAFF: u32 = 20;

    #[test]
    fn a_directory_only_its_owner_may_write_to_is_not_shared() {
        assert_eq!(exposure(ME, STAFF, 0o755, ME), None);
        assert_eq!(exposure(0, 0, 0o755, ME), None);
        assert_eq!(exposure(ME, STAFF, 0o700, ME), None);
    }

    #[test]
    fn a_directory_owned_by_another_user_is_shared() {
        let reason = exposure(502, STAFF, 0o755, ME).unwrap();
        assert_eq!(reason, "is owned by uid 502, neither you nor root");
    }

    #[test]
    fn a_group_or_world_writable_directory_without_the_sticky_bit_is_shared() {
        assert_eq!(
            exposure(ME, STAFF, 0o777, ME).unwrap(),
            "may be written to by every user (mode 777)"
        );
        assert_eq!(
            exposure(0, 0, 0o757, ME).unwrap(),
            "may be written to by every user (mode 757)"
        );
        assert_eq!(
            exposure(ME, STAFF, 0o775, ME).unwrap(),
            "may be written to by its group, gid 20 (mode 775)"
        );
    }

    #[test]
    fn the_sticky_bit_keeps_a_writable_directory_from_being_shared() {
        assert_eq!(exposure(ME, STAFF, 0o1777, ME), None);
        assert_eq!(exposure(0, STAFF, 0o1775, ME), None);
    }

    #[test]
    fn an_administrators_group_may_write_to_it() {
        for &group in ADMIN_GROUPS {
            assert_eq!(exposure(0, group, 0o775, ME), None, "{group}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn applications_as_macos_ships_it_is_not_shared() {
        // `/Applications`: `root:admin`, mode 0775.
        assert_eq!(exposure(0, 80, 0o40775, ME), None);
    }
}
