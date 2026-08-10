//! [`CodexAdapterFactory`]: the composition root's lazy handle to a Codex
//! [`AgentAdapter`].
//!
//! The factory carries only a [`CodexLaunchConfig`], so constructing it has no
//! side effects — no `codex app-server` process is spawned and no handshake is
//! run. That work is deferred to [`AgentAdapterFactory::connect`], called when a
//! Codex session first needs the adapter, so a machine without Codex installed
//! still boots normally.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use delta_usecase::{
    AgentAdapter, AgentAdapterFactory, AgentCapabilities, AgentProvider, CommsLogSink,
    Error as UsecaseError, NullCommsLog, Result as UsecaseResult,
};

use crate::{AppServerConnection, CodexAppServerAdapter, CodexLaunchConfig, CODEX_CAPABILITIES};

/// Builds the Codex [`AgentAdapter`] on demand from a held launch config.
#[derive(Clone)]
pub struct CodexAdapterFactory {
    config: CodexLaunchConfig,
    /// Where every connection this factory builds mirrors its frames for the
    /// comms-log inspector. [`NullCommsLog`] unless the composition root
    /// attached one.
    comms_log: Arc<dyn CommsLogSink>,
}

impl std::fmt::Debug for CodexAdapterFactory {
    /// Hand-written because a `dyn` sink is not `Debug`; the launch config is the
    /// only state worth showing anyway.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexAdapterFactory")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl CodexAdapterFactory {
    /// Hold the launch configuration for a later [`AgentAdapterFactory::connect`].
    ///
    /// Purely stores `config`; nothing is spawned here.
    pub fn new(config: CodexLaunchConfig) -> Self {
        Self {
            config,
            comms_log: NullCommsLog::arc(),
        }
    }

    /// Mirror the frames of every connection this factory builds into `sink`, so
    /// the browser's comms-log inspector can tail them.
    pub fn with_comms_log(mut self, sink: Arc<dyn CommsLogSink>) -> Self {
        self.comms_log = sink;
        self
    }
}

#[async_trait]
impl AgentAdapterFactory for CodexAdapterFactory {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Codex
    }

    fn capabilities(&self) -> AgentCapabilities {
        // The same const the built adapter's `capabilities()` returns, so the
        // pre-connect profile can never drift from a running adapter's.
        CODEX_CAPABILITIES
    }

    async fn connect(&self) -> UsecaseResult<Arc<dyn AgentAdapter>> {
        let conn = Arc::new(
            AppServerConnection::spawn(&self.config)
                .map_err(to_usecase_err)?
                .with_comms_log(Arc::clone(&self.comms_log)),
        );
        // `ClientInfo` requires BOTH `name` and `version` (see the vendored
        // schema); the real `codex app-server` rejects an `initialize` missing
        // `version` with `[-32600] Invalid request: missing field 'version'`.
        // The client version reported to Codex is Delta's own crate version.
        conn.initialize(json!({
            "clientInfo": { "name": "delta", "version": env!("CARGO_PKG_VERSION") }
        }))
        .await
        .map_err(to_usecase_err)?;
        Ok(Arc::new(CodexAppServerAdapter::new(conn)))
    }
}

/// Map a transport error into the use-case error type at the trait boundary,
/// mirroring the adapter's own boundary conversion.
fn to_usecase_err(err: crate::Error) -> UsecaseError {
    UsecaseError::Agent(err.to_string())
}
