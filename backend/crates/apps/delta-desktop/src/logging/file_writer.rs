use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

use super::{LogFileUnavailable, OwnerOnlyAppender};

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
    /// Open the rotating, owner-only log file in `dir`, creating the directory,
    /// behind a non-blocking writer so a write never holds up the thread that
    /// logs.
    pub fn open(dir: Option<&Path>) -> Self {
        let Some(dir) = dir else {
            return Self::StdoutOnly(LogFileUnavailable::NoHomeDirectory);
        };
        match OwnerOnlyAppender::open(dir) {
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
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&files[0]).unwrap().permissions(),
        );
        assert_eq!(
            mode & 0o777,
            super::super::owner_only_appender::LOG_FILE_MODE
        );
        let dir_mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&dir).unwrap().permissions(),
        );
        assert_eq!(dir_mode & 0o777, 0o700);
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
