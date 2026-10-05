//! Composition root.
//!
//! [`build`] constructs the concrete gateways — [`SqliteStore`],
//! [`JsonlTranscript`], [`Tmux`], [`FsWorkspace`] — injects them into an
//! [`Interactor`], and returns the wired application state for the server to
//! drive. This crate is the single place that knows about every concrete
//! implementation; the server depends only on the resulting [`AppInteractor`]
//! alias and the use-case API.
//!
//! [`SqliteStore`]: delta_sqlite::SqliteStore
//! [`JsonlTranscript`]: delta_transcript::JsonlTranscript
//! [`Tmux`]: tmux_driver::Tmux
//! [`FsWorkspace`]: workspace_fs::FsWorkspace
//! [`Interactor`]: delta_usecase::Interactor
//! [`build`]: build()

mod error;
pub use error::{Error, Result};

mod settings;
pub use settings::render_session_settings;

mod launch_option_vocabulary;
pub use launch_option_vocabulary::{
    is_launch_option_dangerous, launch_option_cardinality, GatewayLaunchOptionVocabulary,
};

mod ensure_tmux_available;

mod build;
pub use build::build;

mod config;
pub use config::{Config, DEFAULT_IDENTIFIER};

mod data_layout;
pub use data_layout::DataLayout;

mod data_dir_error;
pub use data_dir_error::DataDirError;

mod launch_option_catalog;
pub use launch_option_catalog::{all_launch_option_presets, launch_option_catalog};

mod provider_capabilities;
pub use provider_capabilities::provider_capabilities;

// Re-export the underlying store error so callers (the `delta-server` binary)
// can pattern-match on its variants — notably `SchemaMismatch`, which it
// surfaces with a clean message at startup — without taking a direct
// dependency on `delta-sqlite`.
pub use delta_sqlite::Error as StoreError;

// Re-exported so a binary that only configures the server (a test harness,
// the `delta-server` main) can name the launch settings without depending on
// the use-case crate directly.
pub use delta_usecase::LaunchConfig;

use delta_usecase::BoxedInteractor;

/// The fully-wired Interactor.
///
/// The gateways are type-erased behind trait objects so the transport layer's
/// shared state is a single non-generic type, shared between this production
/// wiring and the integration tests that substitute fakes.
pub type AppInteractor = BoxedInteractor;
