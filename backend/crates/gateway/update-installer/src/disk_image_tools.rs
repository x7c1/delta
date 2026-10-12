use std::path::Path;

use crate::ToolError;

/// The macOS programs the bundle installer runs: mounting and unmounting
/// the update's disk image, and copying the bundle out of it. A seam, so
/// that the installer's decisions are tested without macOS.
pub trait DiskImageTools: Send + Sync {
    /// Mount the disk image `image` read-only on `mountpoint`, an existing
    /// empty directory, without showing it in Finder or opening anything on
    /// it.
    fn attach(&self, image: &Path, mountpoint: &Path) -> Result<(), ToolError>;

    /// Unmount the disk image mounted on `mountpoint`.
    fn detach(&self, mountpoint: &Path) -> Result<(), ToolError>;

    /// Copy the bundle `from` to `to`, which does not exist yet, keeping its
    /// symlinks, permissions and extended attributes.
    fn copy_bundle(&self, from: &Path, to: &Path) -> Result<(), ToolError>;
}
