//! The connection's and the adapter's unit tests, split by the behaviour they
//! cover. Those that need a server drive an in-process scripted one over a
//! duplex pipe (no subprocess). The end-to-end test against the real fake
//! app-server binary lives in `fake-codex/tests/`.

mod config_merge;
mod connection_death;
mod interrupt;
mod launch_options;
mod resolved_model;
mod transport;
mod worktree_git_grant;

use std::sync::Arc;
use std::time::Duration;

use delta_usecase::{
    AgentAdapter, AgentContentSource, AgentEvent, ContentSourceRequest, Error as UsecaseError,
    LaunchOptionSpec, LaunchRequest, Message, ResumeRequest, SendRequest, SessionId, ThreadId,
};
use serde_json::{json, Value};
use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::sync::mpsc;

use super::*;
use crate::adapter::{thread_resume_params, thread_start_params};

/// A short bound so a wiring bug fails fast instead of hanging the suite.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The server side of a duplex pipe: read client frames line by line, write
/// server frames back.
struct ServerSide {
    reader: BufReader<ReadHalf<tokio::io::DuplexStream>>,
    writer: WriteHalf<tokio::io::DuplexStream>,
}

impl ServerSide {
    async fn next_frame(&mut self) -> Value {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await.unwrap();
        assert_ne!(n, 0, "client closed before the expected frame");
        serde_json::from_str(&line).unwrap()
    }

    async fn send(&mut self, frame: Value) {
        let mut line = serde_json::to_string(&frame).unwrap();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
    }
}

/// Wire a connection to a fresh `ServerSide` over a duplex pipe.
fn connect() -> (AppServerConnection, ServerSide) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (client_r, client_w): (ReadHalf<_>, WriteHalf<_>) = split(client);
    let (server_r, server_w) = split(server);
    let conn = AppServerConnection::from_io(client_r, client_w, None);
    (
        conn,
        ServerSide {
            reader: BufReader::new(server_r),
            writer: server_w,
        },
    )
}

async fn recv(rx: &mut mpsc::UnboundedReceiver<ThreadEvent>) -> ThreadEvent {
    tokio::time::timeout(TIMEOUT, rx.recv())
        .await
        .expect("timed out waiting for a thread event")
        .expect("thread channel closed")
}

/// The unrouted channel is bounded (see `UNROUTED_CAPACITY`), so it has its own
/// receiver type.
async fn recv_unrouted(rx: &mut mpsc::Receiver<ThreadEvent>) -> ThreadEvent {
    tokio::time::timeout(TIMEOUT, rx.recv())
        .await
        .expect("timed out waiting for an unrouted frame")
        .expect("unrouted channel closed")
}

fn option(name: &str, value: Option<&str>) -> LaunchOptionSpec {
    LaunchOptionSpec {
        name: name.to_owned(),
        value: value.map(str::to_owned),
    }
}

/// The dotted config key the grant rides, spelled out here so a test failure
/// shows the exact wire key rather than a constant's name.
const GRANT_KEY: &str = "sandbox_workspace_write.writable_roots";
