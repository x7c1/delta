use std::fs::{DirBuilder, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use chrono::{NaiveDate, Utc};
use tracing_appender::rolling::{RollingFileAppender, Rotation};

use super::KEPT_FILES;

/// The log files are named `delta-desktop.<YYYY-MM-DD>.log`.
const FILE_PREFIX: &str = "delta-desktop";
const FILE_SUFFIX: &str = "log";

/// The log directory's mode: the files record paths, session ids and, at
/// `debug`, prompt texts, so they are the user's alone.
const LOG_DIR_MODE: u32 = 0o700;

/// The mode of every log file.
pub const LOG_FILE_MODE: u32 = 0o600;

/// The daily rotating log file, created owner-only.
///
/// `tracing-appender` creates each day's file with the process's default mode
/// (`0644` under a typical umask) and offers no way to ask for another. So
/// before a write lands on a new day, this creates that day's file itself,
/// `0600`, under the name the appender gives it — the appender then opens the
/// existing file for appending. The day is the UTC date, as the appender's is.
/// Should the two clocks straddle midnight, the file the appender opened first
/// is tightened by the next write.
pub struct OwnerOnlyAppender {
    inner: RollingFileAppender,
    dir: PathBuf,
    /// The day whose file was last made owner-only.
    prepared_for: NaiveDate,
}

impl OwnerOnlyAppender {
    /// Create `dir` owner-only (tightening an existing one: it is named after
    /// the app, so it is Delta's), create today's file in it owner-only, and
    /// open the rotating appender on it.
    pub fn open(dir: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        DirBuilder::new()
            .recursive(true)
            .mode(LOG_DIR_MODE)
            .create(dir)?;
        if std::fs::symlink_metadata(dir)?.is_dir() {
            std::fs::set_permissions(dir, Permissions::from_mode(LOG_DIR_MODE))?;
        }
        let today = Utc::now().date_naive();
        prepare_file(&dir.join(file_name(today)))?;
        let inner = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(FILE_PREFIX)
            .filename_suffix(FILE_SUFFIX)
            .max_log_files(KEPT_FILES)
            .build(dir)?;
        Ok(Self {
            inner,
            dir: dir.to_owned(),
            prepared_for: today,
        })
    }

    /// Make today's file owner-only before the appender writes to it, once a
    /// day. A failure is reported on stderr: this runs on the log's own worker,
    /// which cannot log through `tracing`.
    fn prepare_today(&mut self) {
        let today = Utc::now().date_naive();
        if today == self.prepared_for {
            return;
        }
        // Also tightens the file of the day that just ended, in case the
        // appender opened it before it was prepared.
        for day in [self.prepared_for, today] {
            let path = self.dir.join(file_name(day));
            if let Err(err) = prepare_file(&path) {
                eprintln!(
                    "delta-desktop: could not make {} owner-only: {err}",
                    path.display()
                );
            }
        }
        self.prepared_for = today;
    }
}

impl Write for OwnerOnlyAppender {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.prepare_today();
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// The name `tracing-appender` gives the daily file of `day`.
fn file_name(day: NaiveDate) -> String {
    format!("{FILE_PREFIX}.{}.{FILE_SUFFIX}", day.format("%Y-%m-%d"))
}

/// Create the log file at `path` with [`LOG_FILE_MODE`], or set an existing
/// one to it. A symlink there is not followed.
fn prepare_file(path: &Path) -> io::Result<()> {
    let file = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(LOG_FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.set_permissions(Permissions::from_mode(LOG_FILE_MODE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_name_is_the_appender_s() {
        let day = NaiveDate::from_ymd_opt(2026, 3, 7).unwrap();
        assert_eq!(file_name(day), "delta-desktop.2026-03-07.log");
    }
}
