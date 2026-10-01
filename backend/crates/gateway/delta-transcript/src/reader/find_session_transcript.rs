//! Look a session's transcript up under the transcript root by its file name.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tokio::fs;

use crate::error::Result;

/// The `<root>/<project dir>/<session id>.jsonl` holding `session_id`'s
/// transcript, or `None` when no project directory holds one (see
/// [`delta_usecase::Transcript::find_session_transcript`]).
pub(super) async fn find_session_transcript(
    root: &str,
    session_id: &str,
) -> Result<Option<String>> {
    // The id becomes a file name; one that could name anything other than
    // a plain file directly inside a project directory is not a session's.
    if session_id.is_empty()
        || session_id.contains(std::path::is_separator)
        || session_id.starts_with('.')
    {
        return Ok(None);
    }
    let file_name = format!("{session_id}.jsonl");

    let mut project_dirs = match fs::read_dir(root).await {
        Ok(dirs) => dirs,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut candidates: Vec<(PathBuf, SystemTime)> = Vec::new();
    while let Some(entry) = project_dirs.next_entry().await? {
        let candidate = entry.path().join(&file_name);
        if let Some(modified) = transcript_file_modified(&candidate).await {
            candidates.push((candidate, modified));
        }
    }

    let Some((chosen, _)) = candidates.iter().max_by_key(|(_, modified)| *modified) else {
        return Ok(None);
    };
    if candidates.len() > 1 {
        tracing::info!(
            session_id,
            chosen = %chosen.display(),
            candidates = ?candidates.iter().map(|(path, _)| path.display().to_string()).collect::<Vec<_>>(),
            "several project directories hold the session's transcript; \
             choosing the one written most recently"
        );
    }
    Ok(Some(chosen.to_string_lossy().into_owned()))
}

/// When the transcript file at `path` was last written, or `None` when there is
/// no regular file there. Claude Code only appends to a transcript, so its
/// modification time is when its latest line was written.
///
/// A candidate that exists but cannot be inspected is logged and skipped: one
/// unreadable project directory must not keep the session's real transcript
/// from being found.
async fn transcript_file_modified(path: &Path) -> Option<SystemTime> {
    let metadata = match fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        // A project-directory entry that is a plain file, not a directory.
        Err(e) if e.kind() == std::io::ErrorKind::NotADirectory => return None,
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "skipping a transcript candidate that cannot be inspected"
            );
            return None;
        }
    };
    if !metadata.is_file() {
        return None;
    }
    match metadata.modified() {
        Ok(modified) => Some(modified),
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "skipping a transcript candidate whose modification time is unknown"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use delta_usecase::Transcript;

    use super::*;
    use crate::JsonlTranscript;

    /// Write `<root>/<rel>` (creating its directories) and stamp its
    /// modification time `age_secs` seconds in the past.
    fn write_transcript(root: &Path, rel: &str, age_secs: u64) -> String {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::fs::File::create(&path).unwrap();
        file.set_modified(SystemTime::now() - std::time::Duration::from_secs(age_secs))
            .unwrap();
        path.to_str().unwrap().to_owned()
    }

    /// The session's transcript moved to another project directory: it is
    /// found there, while a subagent's file of the same name under the
    /// session's `subagents/` directory is not a candidate.
    #[tokio::test]
    async fn finds_a_moved_transcript_and_ignores_subagents() {
        let root = tempfile::tempdir().unwrap();
        let moved = write_transcript(root.path(), "-work-wt/sess-1.jsonl", 60);
        // Newer than the real one, so only its location keeps it out.
        write_transcript(root.path(), "-work/sess-1/subagents/sess-1.jsonl", 0);
        write_transcript(root.path(), "-work/sess-2.jsonl", 0);
        // A stray file directly under the root is not a project directory.
        write_transcript(root.path(), "notes.jsonl", 0);

        let found = JsonlTranscript::new()
            .find_session_transcript(root.path().to_str().unwrap(), "sess-1")
            .await
            .unwrap();
        assert_eq!(found.as_deref(), Some(moved.as_str()));
    }

    #[tokio::test]
    async fn picks_the_most_recently_written_of_several() {
        let root = tempfile::tempdir().unwrap();
        write_transcript(root.path(), "-work/sess-1.jsonl", 600);
        let newest = write_transcript(root.path(), "-work-wt-b/sess-1.jsonl", 10);
        write_transcript(root.path(), "-work-wt-a/sess-1.jsonl", 300);

        let found = JsonlTranscript::new()
            .find_session_transcript(root.path().to_str().unwrap(), "sess-1")
            .await
            .unwrap();
        assert_eq!(found.as_deref(), Some(newest.as_str()));
    }

    #[tokio::test]
    async fn finds_nothing_when_no_project_directory_holds_it() {
        let root = tempfile::tempdir().unwrap();
        write_transcript(root.path(), "-work/sess-2.jsonl", 0);
        let t = JsonlTranscript::new();
        let root_str = root.path().to_str().unwrap();

        assert_eq!(
            t.find_session_transcript(root_str, "sess-1").await.unwrap(),
            None
        );
        // A missing root is "nothing found", not an error.
        let gone = root.path().join("absent");
        assert_eq!(
            t.find_session_transcript(gone.to_str().unwrap(), "sess-2")
                .await
                .unwrap(),
            None
        );
        // An id that is not a plain file name never matches.
        assert_eq!(
            t.find_session_transcript(root_str, "../-work/sess-2")
                .await
                .unwrap(),
            None
        );
    }
}
