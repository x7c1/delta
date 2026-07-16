//! Errors surfaced by the `codex app-server` transport.

use crate::wire::RpcError;

/// A transport-level result.
pub type Result<T> = std::result::Result<T, Error>;

/// Something went wrong talking to the app-server.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Spawning the `codex app-server` process failed.
    #[error("failed to spawn app-server: {0}")]
    Spawn(std::io::Error),

    /// The child process did not expose the stdio pipes we asked for.
    #[error("app-server process is missing its {0} pipe")]
    MissingPipe(&'static str),

    /// Writing a frame to the server failed.
    #[error("failed to write to app-server: {0}")]
    Write(std::io::Error),

    /// Serialising an outgoing frame failed.
    #[error("failed to encode outgoing frame: {0}")]
    Encode(serde_json::Error),

    /// The connection closed before the request was answered (the reader task
    /// stopped, typically because the server process exited).
    #[error("app-server connection closed before the request was answered")]
    ConnectionClosed,

    /// The server answered the request with a JSON-RPC error object.
    #[error("app-server returned an error for `{method}`: {error}")]
    Rpc {
        /// The method whose call failed.
        method: String,
        /// The JSON-RPC error the server returned.
        error: RpcError,
    },

    /// The server's response did not have the shape this call expected (e.g. a
    /// `thread/start` result without a thread id).
    #[error("unexpected response shape for `{method}`: {detail}")]
    UnexpectedResponse {
        /// The method whose response could not be interpreted.
        method: String,
        /// What was wrong with it.
        detail: String,
    },
}
