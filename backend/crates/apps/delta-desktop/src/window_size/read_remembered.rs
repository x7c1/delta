use std::io;
use std::path::Path;

use super::{is_usable, SavedSize};

/// The size remembered in the state file at `path`, with its screen. A file
/// that is missing, cannot be read or parsed, or holds an unusable size means
/// a first launch; an unusable screen, or none (a file from before Delta kept
/// it), is read as unknown.
pub fn read_remembered(path: &Path) -> Option<SavedSize> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
        Err(err) => {
            tracing::warn!(path = %path.display(), "could not read the window size file: {err}");
            return None;
        }
    };
    let mut saved = match serde_json::from_str::<SavedSize>(&contents) {
        Ok(saved) => saved,
        Err(err) => {
            tracing::warn!(path = %path.display(), "the window size file is not valid: {err}");
            return None;
        }
    };
    let size = saved.size();
    if !is_usable(size) {
        tracing::warn!(
            path = %path.display(),
            width = size.width,
            height = size.height,
            "the window size file holds no usable size"
        );
        return None;
    }
    saved.screen = saved.screen.filter(|screen| is_usable(*screen));
    Some(saved)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{saved, saved_on, size};
    use super::super::STATE_FILE_NAME;
    use super::*;

    #[test]
    fn a_file_without_the_screen_reads_as_a_size_from_an_unknown_screen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE_NAME);
        for contents in [
            r#"{"width":1366,"height":1659}"#,
            r#"{"width":1366,"height":1659,"screen":{"width":0,"height":1728}}"#,
        ] {
            std::fs::write(&path, contents).unwrap();
            assert_eq!(
                read_remembered(&path),
                Some(saved(1366.0, 1659.0)),
                "contents: {contents}"
            );
        }
    }

    #[test]
    fn a_file_with_the_screen_reads_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE_NAME);
        std::fs::write(
            &path,
            r#"{"width":1366,"height":1659,"screen":{"width":4096,"height":1728}}"#,
        )
        .unwrap();
        assert_eq!(
            read_remembered(&path),
            Some(saved_on(1366.0, 1659.0, size(4096.0, 1728.0)))
        );
    }

    #[test]
    fn a_missing_file_means_a_first_launch() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_remembered(&dir.path().join(STATE_FILE_NAME)), None);
    }

    #[test]
    fn an_unreadable_or_invalid_file_means_a_first_launch() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be cannot be read as one.
        let unreadable = dir.path().join("unreadable");
        std::fs::create_dir(&unreadable).unwrap();
        assert_eq!(read_remembered(&unreadable), None);

        let path = dir.path().join(STATE_FILE_NAME);
        for contents in [
            "",
            "{\"width\":",
            r#"{"main":{"width":2400,"height":1500}}"#,
            r#"{"width":0,"height":800}"#,
            r#"{"width":-1200,"height":800}"#,
        ] {
            std::fs::write(&path, contents).unwrap();
            assert_eq!(read_remembered(&path), None, "contents: {contents}");
        }
    }
}
