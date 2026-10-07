use std::io;
use std::path::Path;

use super::SavedSize;

/// Write `saved` to the state file at `path`, creating its directory.
pub fn write_remembered(path: &Path, saved: SavedSize) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(&saved)?)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{saved, saved_on, size};
    use super::super::{read_remembered, STATE_FILE_NAME};
    use super::*;

    #[test]
    fn a_saved_size_reads_back_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        // The config directory may not exist yet on a first exit.
        let path = dir.path().join("config").join(STATE_FILE_NAME);
        for remembered in [
            saved_on(1418.0, 1748.5, size(1706.6666666666667, 1080.0)),
            saved(1418.0, 1748.5),
        ] {
            write_remembered(&path, remembered).unwrap();
            assert_eq!(read_remembered(&path), Some(remembered));
        }
    }
}
