//! `POST /api/storage/erase`: erasing everything, stopping the server, and
//! the server deleting its data directory once the store is closed.

use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::serve::{self, ServerStopped};
use axum::http::StatusCode;

/// What a running server leaves in its data directory besides the database,
/// which the store itself creates.
const PLANTED: [&str; 5] = [
    "sessions/tok/notes.txt",
    "settings/7878.json",
    "tmux.conf",
    "delta.db.bak-v2",
    "delta-hook-state.json",
];

/// A state over a data directory and a worktree base of its own under `root`,
/// on a tmux socket no other test or server uses, with [`PLANTED`] in its data
/// directory.
async fn erasable_state(root: &Path) -> AppState {
    let data_dir = root.join("data");
    let socket = format!("delta-test-erase-{}", uuid::Uuid::new_v4().simple());
    let config = Config {
        data_dir: data_dir.to_string_lossy().into_owned(),
        worktree_base: root.join("worktrees").to_string_lossy().into_owned(),
        tmux_socket: socket,
        ..test_config()
    };
    config.data_layout().create_dirs().unwrap();
    let state = AppState::build(&config).await.unwrap();
    for name in PLANTED {
        let path = data_dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }
    assert!(data_dir.join("delta.db").exists(), "the store is open");
    state
}

/// Send one bodiless `POST` over a real connection and read the status and
/// the JSON body.
async fn post(port: u16, path: &str) -> (u16, serde_json::Value) {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {}\r\n\
         Content-Length: 0\r\nConnection: close\r\n\r\n",
        bearer()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let status = response[9..12].parse().unwrap();
    let (_, body) = response.split_once("\r\n\r\n").unwrap();
    (status, serde_json::from_str(body).unwrap())
}

/// The erase answers with its report and stops the server, which then — with
/// the store closed — deletes the data directory, planted files, database and
/// all, and `serve` returns why it stopped.
#[tokio::test]
async fn erasing_answers_with_the_report_then_stops_and_deletes_the_data_directory() {
    let root = tempfile::tempdir().unwrap();
    let state = erasable_state(root.path()).await;
    let data_dir = root.path().join("data");
    let listener = serve::bind_loopback(0).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(serve::serve(state, listener));

    let (status, body) = post(port, "/api/storage/erase").await;

    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        serde_json::json!({
            "removed": {
                "sessions": 0,
                "worktrees": [],
                "branches": [],
                "data_dir": data_dir.to_string_lossy(),
            },
            "kept": [],
        })
    );
    let stopped = tokio::time::timeout(std::time::Duration::from_secs(20), server)
        .await
        .expect("the server stops")
        .unwrap()
        .unwrap();
    assert!(matches!(stopped, ServerStopped::Erased(_)), "{stopped:?}");
    assert!(!data_dir.exists(), "the data directory is gone");
    assert!(root.path().exists(), "nothing above it is touched");
}

/// A second erase while the first runs — or after it, while the server stops —
/// is refused with its code, and does nothing.
#[tokio::test]
async fn a_second_erase_is_a_conflict() {
    let root = tempfile::tempdir().unwrap();
    let state = erasable_state(root.path()).await;

    let (first, second) = tokio::join!(
        request_json(&state, "POST", "/api/storage/erase", None),
        request_json(&state, "POST", "/api/storage/erase", None),
    );

    let mut statuses = [first.0, second.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    let refused = if first.0 == StatusCode::CONFLICT {
        first.1
    } else {
        second.1
    };
    assert_eq!(refused["code"], "erase_in_progress");
}
