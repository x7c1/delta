//! Capability traits (ports) and the data they exchange.
//!
//! These traits describe what Delta needs from the outside world. The gateway
//! crates implement them; the [`crate::Interactor`] consumes them. Everything
//! here is expressed in terms of [`delta_model`] types only.

mod new_session;
pub use new_session::NewSession;
mod session_event;
pub use session_event::SessionEvent;
mod session_lifecycle;
pub use session_lifecycle::SessionLifecycle;
mod session_store;
pub use session_store::SessionStore;
mod stop_hook;
pub use stop_hook::StopHook;
mod tmux_driver;
pub use tmux_driver::TmuxDriver;
mod transcript;
pub use transcript::Transcript;
mod transcript_message;
pub use transcript_message::TranscriptMessage;
mod transcript_read;
pub use transcript_read::TranscriptRead;
mod user_prompt_submit_hook;
pub use user_prompt_submit_hook::UserPromptSubmitHook;
mod workspace;
pub use workspace::Workspace;
