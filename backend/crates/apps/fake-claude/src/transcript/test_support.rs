//! Helpers shared by the transcript writer's tests.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub(super) fn temp_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("fake-claude-transcript-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{name}-{}.jsonl", std::process::id()))
}

pub(super) fn read_lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
