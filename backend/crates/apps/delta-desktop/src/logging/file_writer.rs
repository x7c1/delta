use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};

use super::{LogFileUnavailable, KEPT_FILES};

/// The log files are named `delta-desktop.<YYYY-MM-DD>.log`.
const FILE_PREFIX: &str = "delta-desktop";
const FILE_SUFFIX: &str = "log";

/// Where the log goes besides stdout.
pub enum FileWriter {
    /// Into the daily files in this directory, through `writer`, whose worker
    /// flushes and stops when `guard` drops.
    Writing {
        dir: PathBuf,
        writer: NonBlocking,
        guard: WorkerGuard,
    },
    /// Nowhere, for the reason given, to be logged at `warn` once logging is
    /// up.
    StdoutOnly(LogFileUnavailable),
}

impl FileWriter {
    /// Open the rotating log file in `dir`, creating the directory, behind a
    /// non-blocking writer so a write never holds up the thread that logs.
    pub fn open(dir: Option<&Path>) -> Self {
        let Some(dir) = dir else {
            return Self::StdoutOnly(LogFileUnavailable::NoHomeDirectory);
        };
        let appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(FILE_PREFIX)
            .filename_suffix(FILE_SUFFIX)
            .max_log_files(KEPT_FILES)
            .build(dir);
        match appender {
            Ok(appender) => {
                let (writer, guard) = tracing_appender::non_blocking(appender);
                Self::Writing {
                    dir: dir.to_owned(),
                    writer,
                    guard,
                }
            }
            Err(cause) => Self::StdoutOnly(LogFileUnavailable::Unwritable {
                dir: dir.to_owned(),
                cause,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tracing_subscriber::fmt;

    #[test]
    fn the_log_file_is_created_and_receives_a_line() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("logs");

        let FileWriter::Writing { writer, guard, .. } = FileWriter::open(Some(&dir)) else {
            panic!("the log file should open in a writable directory");
        };
        let subscriber = fmt().with_writer(writer).with_ansi(false).finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("reaches the log file");
        });
        // Flushes the worker, as the app does on exit.
        drop(guard);

        let files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1, "{files:?}");
        let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("delta-desktop.") && name.ends_with(".log"),
            "{name}"
        );
        let logged = std::fs::read_to_string(&files[0]).unwrap();
        assert!(logged.contains("reaches the log file"), "{logged:?}");
    }

    #[test]
    fn an_unwritable_directory_falls_back_to_stdout_only() {
        let temp = tempfile::tempdir().unwrap();
        // A directory cannot be made under a regular file, whoever runs this.
        let blocker = temp.path().join("not-a-directory");
        std::fs::write(&blocker, "").unwrap();

        let dir = blocker.join("logs");

        match FileWriter::open(Some(&dir)) {
            FileWriter::StdoutOnly(LogFileUnavailable::Unwritable { dir: named, .. }) => {
                assert_eq!(named, dir);
            }
            FileWriter::StdoutOnly(other) => panic!("not an unwritable directory: {other:?}"),
            FileWriter::Writing { .. } => panic!("there is no directory to write to"),
        }
    }

    #[test]
    fn no_log_directory_falls_back_to_stdout_only() {
        assert!(matches!(
            FileWriter::open(None),
            FileWriter::StdoutOnly(LogFileUnavailable::NoHomeDirectory)
        ));
    }
}
