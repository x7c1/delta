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
    AgentAdapter, AgentAdapterFactory, AgentProvider, Error as UsecaseError,
    Result as UsecaseResult,
};

use crate::{AppServerConnection, CodexAppServerAdapter, CodexLaunchConfig};

/// Builds the Codex [`AgentAdapter`] on demand from a held launch config.
#[derive(Debug, Clone)]
pub struct CodexAdapterFactory {
    config: CodexLaunchConfig,
}

impl CodexAdapterFactory {
    /// Hold the launch configuration for a later [`AgentAdapterFactory::connect`].
    ///
    /// Purely stores `config`; nothing is spawned here.
    pub fn new(config: CodexLaunchConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl AgentAdapterFactory for CodexAdapterFactory {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Codex
    }

    async fn connect(&self) -> UsecaseResult<Arc<dyn AgentAdapter>> {
        let conn = Arc::new(AppServerConnection::spawn(&self.config).map_err(to_usecase_err)?);
        conn.initialize(json!({ "clientInfo": { "name": "delta" } }))
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
