use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use delta_usecase::{InstallError, InstalledApp, UpdateInstaller};

use crate::image_check::{check_image, APP_NAME};
use crate::swap_paths::{remove_any, SwapPaths};
use crate::verified_copy::copy_verified;
use crate::{BundleLocation, DiskImageTools, SystemDiskImageTools, ToolError};

/// The macOS [`UpdateInstaller`]: replaces the running `Delta.app` with the
/// one in the downloaded disk image, as the user running Delta.
///
/// Nothing runs as root and nothing asks for a password: where this user may
/// not write next to the bundle, Delta cannot install the update and says so
/// ([`InstallError::Unavailable`]). The steps, each mapped to an outcome, are
/// in the [crate documentation](crate).
pub struct BundleInstaller {
    /// Where the app runs from, resolved once when the installer was made,
    /// before any update replaced anything; `Err` says why it could not be
    /// resolved.
    location: Result<BundleLocation, String>,
    tools: Arc<dyn DiskImageTools>,
    /// The directory the private working directory of each install is made
    /// in.
    temp_dir: PathBuf,
}

impl BundleInstaller {
    /// The installer of the running app: where it runs from, as this
    /// process's executable says now, through `hdiutil` and `ditto`.
    pub fn for_running_app() -> Self {
        let location = std::env::current_exe()
            .map(|exe| {
                // Through the links the path was started by, to the bundle
                // that holds it.
                let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
                BundleLocation::of_executable(&exe)
            })
            .map_err(|err| format!("could not tell where Delta runs from: {err}"));
        tracing::debug!(location = ?location, "where the running app is, for its updates");
        Self {
            location,
            tools: Arc::new(SystemDiskImageTools),
            temp_dir: std::env::temp_dir(),
        }
    }

    /// An installer of the app at `location`, through `tools`, working in a
    /// new directory under `temp_dir` for each install.
    #[cfg(test)]
    pub(crate) fn with(
        location: BundleLocation,
        tools: Arc<dyn DiskImageTools>,
        temp_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            location: Ok(location),
            tools,
            temp_dir: temp_dir.into(),
        }
    }

    /// Remove what an earlier update left next to the running bundle: the
    /// bundle it replaced, kept until now since the app ran from it, and a
    /// copy it never put in place. Nothing when the app does not run from a
    /// bundle it could update.
    pub fn remove_update_leftovers(&self) {
        if let Ok(BundleLocation::Bundle(bundle)) = &self.location {
            if let Some(paths) = SwapPaths::of(bundle) {
                paths.remove_leftovers();
            }
        }
    }
}

#[async_trait]
impl UpdateInstaller for BundleInstaller {
    async fn install(
        &self,
        version: &str,
        file: &Path,
        sha256: &str,
    ) -> Result<InstalledApp, InstallError> {
        let paths = replaceable_bundle(&self.location).map_err(InstallError::Unavailable)?;
        let tools = Arc::clone(&self.tools);
        let temp_dir = self.temp_dir.clone();
        let (version, file, sha256) = (version.to_owned(), file.to_path_buf(), sha256.to_owned());
        tokio::task::spawn_blocking(move || {
            install_bundle(tools.as_ref(), &temp_dir, &paths, &version, &file, &sha256)
        })
        .await
        .map_err(|err| InstallError::Failed(format!("the install stopped unexpectedly: {err}")))?
    }

    /// Why no update can replace the running bundle: it is not in a bundle,
    /// macOS runs a translocated copy of it, or this user may not write to
    /// its directory. Checked without writing anything.
    fn unavailable(&self) -> Option<String> {
        replaceable_bundle(&self.location).err()
    }
}

/// Where the bundle at `location` would be updated, or why Delta cannot
/// update it ([`InstallError::Unavailable`]'s text): it is not in a bundle,
/// macOS runs a translocated copy of it, or this user may not write to the
/// bundle's directory. Writes nothing.
fn replaceable_bundle(location: &Result<BundleLocation, String>) -> Result<SwapPaths, String> {
    let bundle = match location {
        Ok(BundleLocation::Bundle(bundle)) => bundle,
        Ok(BundleLocation::Translocated(_)) => {
            return Err(
                "macOS runs Delta from a read-only copy (App Translocation), since it was \
                 opened from its disk image or the folder it was downloaded to: move Delta to \
                 /Applications"
                    .to_owned(),
            )
        }
        Ok(BundleLocation::NotInBundle(exe)) => {
            return Err(format!(
                "Delta does not run from an app bundle ({}), so there is no {APP_NAME} to replace",
                exe.display()
            ))
        }
        Err(reason) => return Err(reason.clone()),
    };
    let paths = SwapPaths::of(bundle)
        .ok_or_else(|| format!("{} has no parent directory", bundle.display()))?;
    if !writable(&paths.dir) {
        return Err(format!(
            "you cannot write to {}, where {} is, so Delta cannot replace it",
            paths.dir.display(),
            bundle.display()
        ));
    }
    Ok(paths)
}

/// Whether this process may create and rename entries in the directory
/// `dir` (`access(2)` with `W_OK`, which counts the groups the user is in).
fn writable(dir: &Path) -> bool {
    let Ok(path) = CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is a NUL-terminated string that outlives the call, and
    // `access` only reads it.
    unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
}

/// Install the disk image `file` of release `version` over the bundle at
/// `paths`, working in a new private directory under `temp_dir`: copy the
/// image there checking its `sha256`, mount the copy, check and copy out its
/// `Delta.app`, unmount it, and swap the copy for the bundle.
fn install_bundle(
    tools: &dyn DiskImageTools,
    temp_dir: &Path,
    paths: &SwapPaths,
    version: &str,
    file: &Path,
    sha256: &str,
) -> Result<InstalledApp, InstallError> {
    // Made 0700: only this user reaches the copy and the mount point.
    let work = tempfile::Builder::new()
        .prefix("delta-update-")
        .tempdir_in(temp_dir)
        .map_err(|err| {
            InstallError::Failed(format!(
                "could not make a working directory in {}: {err}",
                temp_dir.display()
            ))
        })?;
    let image = work.path().join("update.dmg");
    copy_verified(file, sha256, &image).map_err(|err| {
        // A copy that could not be written says nothing about the file,
        // which stays to be installed by hand or retried.
        if err.rejects_file() {
            InstallError::Rejected(err.to_string())
        } else {
            InstallError::Failed(err.to_string())
        }
    })?;
    let mountpoint = work.path().join("mount");
    std::fs::create_dir(&mountpoint).map_err(|err| {
        InstallError::Failed(format!(
            "could not make the mount point {}: {err}",
            mountpoint.display()
        ))
    })?;

    stage_from_image(tools, &image, &mountpoint, version, &paths.staging)?;
    if let Err(reason) = paths.swap() {
        discard_staging(&paths.staging);
        return Err(InstallError::Failed(reason));
    }
    tracing::info!(
        bundle = %paths.bundle.display(),
        backup = %paths.backup.display(),
        version,
        "replaced the app bundle with the update's"
    );
    Ok(InstalledApp::Bundle(paths.bundle.clone()))
}

/// Mount `image` on `mountpoint`, check what it holds, and copy its
/// `Delta.app` to `staging`. Once the image is attached it is always
/// detached again, whatever happened in between; a detach that fails is
/// logged, as the copy is done by then.
fn stage_from_image(
    tools: &dyn DiskImageTools,
    image: &Path,
    mountpoint: &Path,
    version: &str,
    staging: &Path,
) -> Result<(), InstallError> {
    tools.attach(image, mountpoint).map_err(tool_failure)?;
    let staged = check_image(mountpoint, version)
        .map_err(InstallError::Rejected)
        .and_then(|app| copy_app(tools, &app, staging));
    if let Err(err) = tools.detach(mountpoint) {
        tracing::warn!(
            mountpoint = %mountpoint.display(),
            error = %err,
            "could not detach the update's disk image"
        );
    }
    staged
}

/// Copy the checked `app` to `staging`, replacing what an earlier update
/// may have left there. A partial copy is removed.
fn copy_app(tools: &dyn DiskImageTools, app: &Path, staging: &Path) -> Result<(), InstallError> {
    remove_any(staging).map_err(|err| {
        InstallError::Failed(format!("could not remove {}: {err}", staging.display()))
    })?;
    tools.copy_bundle(app, staging).map_err(|err| {
        discard_staging(staging);
        tool_failure(err)
    })
}

/// Remove the staged copy of an update that was not put in place, logging a
/// failure: the next launch removes it otherwise.
fn discard_staging(staging: &Path) {
    if let Err(err) = remove_any(staging) {
        tracing::warn!(
            path = %staging.display(),
            error = %err,
            "could not remove the copy of the update that was not put in place"
        );
    }
}

/// What a disk image tool failing means for the install: a missing tool is
/// a machine Delta cannot install on, one that cannot be started too; one
/// that ran and failed is a failed install.
fn tool_failure(err: ToolError) -> InstallError {
    match err {
        ToolError::Missing { .. } => InstallError::Unavailable(err.to_string()),
        ToolError::Unrunnable { program, source } => InstallError::Unrunnable { program, source },
        ToolError::Failed { .. } => InstallError::Failed(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use sha2::{Digest, Sha256};

    use super::*;
    use crate::testing::{write_app, write_image};

    const DMG: &[u8] = b"the v0.6.0 disk image";

    fn dmg_sha256() -> String {
        Sha256::digest(DMG)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Disk image tools that record what they were asked, mount a fake
    /// image (`image` fills the mount point), copy with `cp`, and fail where
    /// told to.
    struct FakeTools {
        calls: Mutex<Vec<&'static str>>,
        image: Box<dyn Fn(&Path) + Send + Sync>,
        attach: fn() -> Result<(), ToolError>,
        copy: fn() -> Result<(), ToolError>,
    }

    impl FakeTools {
        /// Tools mounting a Delta disk image of `version`.
        fn of_version(version: &'static str) -> Self {
            Self::with_image(move |root| write_image(root, version))
        }

        fn with_image(image: impl Fn(&Path) + Send + Sync + 'static) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                image: Box::new(image),
                attach: || Ok(()),
                copy: || Ok(()),
            }
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl DiskImageTools for FakeTools {
        fn attach(&self, image: &Path, mountpoint: &Path) -> Result<(), ToolError> {
            self.calls.lock().unwrap().push("attach");
            assert_eq!(std::fs::read(image).unwrap(), DMG, "the verified copy");
            (self.attach)()?;
            (self.image)(mountpoint);
            Ok(())
        }

        fn detach(&self, _mountpoint: &Path) -> Result<(), ToolError> {
            self.calls.lock().unwrap().push("detach");
            Ok(())
        }

        fn copy_bundle(&self, from: &Path, to: &Path) -> Result<(), ToolError> {
            self.calls.lock().unwrap().push("copy");
            (self.copy)()?;
            let status = std::process::Command::new("cp")
                .arg("-R")
                .arg(from)
                .arg(to)
                .status()
                .unwrap();
            assert!(status.success());
            Ok(())
        }
    }

    /// A machine with Delta 0.5.0 at `<dir>/apps/Delta.app` and the
    /// verified download of 0.6.0 at `<dir>/updates/Delta_0.6.0_aarch64.dmg`.
    struct Machine {
        dir: tempfile::TempDir,
    }

    impl Machine {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            write_app(
                &dir.path().join("apps/Delta.app"),
                crate::image_check::BUNDLE_IDENTIFIER,
                "0.5.0",
            );
            std::fs::create_dir(dir.path().join("updates")).unwrap();
            std::fs::write(dir.path().join("updates/Delta_0.6.0_aarch64.dmg"), DMG).unwrap();
            std::fs::create_dir(dir.path().join("tmp")).unwrap();
            Self { dir }
        }

        fn bundle(&self) -> PathBuf {
            self.dir.path().join("apps/Delta.app")
        }

        fn download(&self) -> PathBuf {
            self.dir.path().join("updates/Delta_0.6.0_aarch64.dmg")
        }

        fn installer(&self, tools: &Arc<FakeTools>) -> BundleInstaller {
            BundleInstaller::with(
                BundleLocation::Bundle(self.bundle()),
                Arc::clone(tools) as Arc<dyn DiskImageTools>,
                self.dir.path().join("tmp"),
            )
        }

        async fn install(&self, tools: &Arc<FakeTools>) -> Result<InstalledApp, InstallError> {
            self.installer(tools)
                .install("v0.6.0", &self.download(), &dmg_sha256())
                .await
        }

        /// The version the app at `bundle` states.
        fn version_of(bundle: &Path) -> String {
            std::fs::read_to_string(bundle.join("Contents/MacOS/delta-desktop")).unwrap()
        }

        /// Whether the install left its working directory behind.
        fn work_left(&self) -> bool {
            std::fs::read_dir(self.dir.path().join("tmp"))
                .unwrap()
                .next()
                .is_some()
        }
    }

    #[tokio::test]
    async fn an_install_replaces_the_bundle_and_keeps_the_old_one_as_the_backup() {
        let machine = Machine::new();
        let tools = Arc::new(FakeTools::of_version("0.6.0"));
        let app = machine.install(&tools).await.unwrap();
        assert_eq!(app, InstalledApp::Bundle(machine.bundle()));
        assert_eq!(Machine::version_of(&machine.bundle()), "0.6.0");
        let backup = machine.dir.path().join("apps/.delta-update-backup");
        assert_eq!(Machine::version_of(&backup), "0.5.0");
        assert_eq!(tools.calls(), ["attach", "copy", "detach"]);
        assert!(!machine.dir.path().join("apps/.delta-update-new").exists());
        assert!(!machine.work_left());

        // The next launch removes the backup.
        machine.installer(&tools).remove_update_leftovers();
        assert!(!backup.exists());
        assert_eq!(Machine::version_of(&machine.bundle()), "0.6.0");
    }

    #[tokio::test]
    async fn an_image_failing_its_checks_is_rejected_and_detached() {
        for (image, expected) in [
            (
                FakeTools::of_version("0.5.0"),
                "the disk image's Delta.app is version 0.5.0, not 0.6.0",
            ),
            (
                FakeTools::with_image(|_| {}),
                "the disk image holds no Delta.app",
            ),
        ] {
            let machine = Machine::new();
            let tools = Arc::new(image);
            match machine.install(&tools).await {
                Err(InstallError::Rejected(reason)) => assert_eq!(reason, expected),
                other => panic!("{other:?}"),
            }
            assert_eq!(tools.calls(), ["attach", "detach"]);
            assert_eq!(Machine::version_of(&machine.bundle()), "0.5.0");
            assert!(!machine.work_left());
        }
    }

    #[tokio::test]
    async fn a_failed_copy_is_detached_and_leaves_the_bundle() {
        let machine = Machine::new();
        let mut tools = FakeTools::of_version("0.6.0");
        tools.copy = || {
            Err(ToolError::Failed {
                program: PathBuf::from("/usr/bin/ditto"),
                said: "ditto: No space left on device".to_owned(),
            })
        };
        let tools = Arc::new(tools);
        match machine.install(&tools).await {
            Err(InstallError::Failed(reason)) => {
                assert_eq!(
                    reason,
                    "/usr/bin/ditto failed: ditto: No space left on device"
                )
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(tools.calls(), ["attach", "copy", "detach"]);
        assert_eq!(Machine::version_of(&machine.bundle()), "0.5.0");
        assert!(!machine.dir.path().join("apps/.delta-update-new").exists());
    }

    #[tokio::test]
    async fn a_failed_or_missing_hdiutil_is_never_detached() {
        let machine = Machine::new();
        let mut failing = FakeTools::of_version("0.6.0");
        failing.attach = || {
            Err(ToolError::Failed {
                program: PathBuf::from("/usr/bin/hdiutil"),
                said: "hdiutil: attach failed - no mountable file systems".to_owned(),
            })
        };
        let failing = Arc::new(failing);
        assert!(matches!(
            machine.install(&failing).await,
            Err(InstallError::Failed(reason)) if reason.contains("no mountable file systems")
        ));
        assert_eq!(failing.calls(), ["attach"]);

        let mut missing = FakeTools::of_version("0.6.0");
        missing.attach = || {
            Err(ToolError::Missing {
                program: PathBuf::from("/usr/bin/hdiutil"),
            })
        };
        let missing = Arc::new(missing);
        assert!(matches!(
            machine.install(&missing).await,
            Err(InstallError::Unavailable(reason)) if reason == "/usr/bin/hdiutil is not installed"
        ));
        assert_eq!(Machine::version_of(&machine.bundle()), "0.5.0");
        assert!(!machine.work_left());
    }

    #[tokio::test]
    async fn a_mismatching_symlinked_or_odd_file_is_rejected_before_mounting() {
        let machine = Machine::new();
        let tools = Arc::new(FakeTools::of_version("0.6.0"));
        let installer = machine.installer(&tools);

        let mismatch = installer
            .install("v0.6.0", &machine.download(), &"0".repeat(64))
            .await;
        assert!(
            matches!(&mismatch, Err(InstallError::Rejected(reason)) if reason.contains("sha256")),
            "{mismatch:?}"
        );

        let link = machine.dir.path().join("updates/link.dmg");
        std::os::unix::fs::symlink(machine.download(), &link).unwrap();
        let symlinked = installer.install("v0.6.0", &link, &dmg_sha256()).await;
        assert!(
            matches!(&symlinked, Err(InstallError::Rejected(reason)) if reason.contains("symlink")),
            "{symlinked:?}"
        );

        let directory = machine.dir.path().join("updates/directory.dmg");
        std::fs::create_dir(&directory).unwrap();
        let odd = installer.install("v0.6.0", &directory, &dmg_sha256()).await;
        assert!(
            matches!(&odd, Err(InstallError::Rejected(reason)) if reason.contains("not a regular file")),
            "{odd:?}"
        );

        assert!(tools.calls().is_empty());
        assert_eq!(Machine::version_of(&machine.bundle()), "0.5.0");
        assert!(!machine.work_left());
    }

    #[tokio::test]
    async fn an_app_not_in_a_bundle_translocated_or_not_writable_is_unavailable() {
        let machine = Machine::new();
        let tools = Arc::new(FakeTools::of_version("0.6.0"));
        let at = |location| {
            BundleInstaller::with(
                location,
                Arc::clone(&tools) as Arc<dyn DiskImageTools>,
                machine.dir.path().join("tmp"),
            )
        };
        let install = |installer: BundleInstaller| {
            let download = machine.download();
            async move { installer.install("v0.6.0", &download, &dmg_sha256()).await }
        };

        let not_in_bundle = install(at(BundleLocation::NotInBundle(PathBuf::from(
            "/usr/local/bin/delta-server",
        ))))
        .await;
        assert!(
            matches!(&not_in_bundle, Err(InstallError::Unavailable(reason)) if reason.contains("does not run from an app bundle")),
            "{not_in_bundle:?}"
        );

        let translocated = install(at(BundleLocation::Translocated(PathBuf::from(
            "/private/var/folders/xy/T/AppTranslocation/0F1E/d/Delta.app",
        ))))
        .await;
        assert!(
            matches!(&translocated, Err(InstallError::Unavailable(reason)) if reason.contains("App Translocation") && reason.contains("move Delta to /Applications")),
            "{translocated:?}"
        );

        // Root may write anywhere, so only another user sees the refusal.
        if unsafe { libc::geteuid() } != 0 {
            let apps = machine.dir.path().join("apps");
            std::fs::set_permissions(&apps, std::os::unix::fs::PermissionsExt::from_mode(0o555))
                .unwrap();
            let read_only = install(at(BundleLocation::Bundle(machine.bundle()))).await;
            std::fs::set_permissions(&apps, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
            assert!(
                matches!(&read_only, Err(InstallError::Unavailable(reason)) if reason.contains("you cannot write to")),
                "{read_only:?}"
            );
        }

        assert!(tools.calls().is_empty());
        assert_eq!(Machine::version_of(&machine.bundle()), "0.5.0");
    }

    /// What startup asks: the same checks as an install's first steps, with
    /// nothing run and nothing written.
    #[test]
    fn startup_tells_a_replaceable_bundle_from_one_that_is_not() {
        let machine = Machine::new();
        let tools = Arc::new(FakeTools::of_version("0.6.0"));
        let unavailable = |location| {
            BundleInstaller::with(
                location,
                Arc::clone(&tools) as Arc<dyn DiskImageTools>,
                machine.dir.path().join("tmp"),
            )
            .unavailable()
        };

        assert_eq!(unavailable(BundleLocation::Bundle(machine.bundle())), None);
        let not_in_bundle = unavailable(BundleLocation::NotInBundle(PathBuf::from(
            "/usr/local/bin/delta-server",
        )));
        assert!(
            not_in_bundle
                .as_deref()
                .is_some_and(|reason| reason.contains("does not run from an app bundle")),
            "{not_in_bundle:?}"
        );
        let translocated = unavailable(BundleLocation::Translocated(PathBuf::from(
            "/private/var/folders/xy/T/AppTranslocation/0F1E/d/Delta.app",
        )));
        assert!(
            translocated
                .as_deref()
                .is_some_and(|reason| reason.contains("App Translocation")),
            "{translocated:?}"
        );
        if unsafe { libc::geteuid() } != 0 {
            let apps = machine.dir.path().join("apps");
            std::fs::set_permissions(&apps, std::os::unix::fs::PermissionsExt::from_mode(0o555))
                .unwrap();
            let read_only = unavailable(BundleLocation::Bundle(machine.bundle()));
            std::fs::set_permissions(&apps, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
            assert!(
                read_only
                    .as_deref()
                    .is_some_and(|reason| reason.contains("you cannot write to")),
                "{read_only:?}"
            );
        }

        assert!(tools.calls().is_empty());
        assert!(!machine.work_left());
        assert_eq!(Machine::version_of(&machine.bundle()), "0.5.0");
    }
}
