//! Rebuilding the running-subagent set from the persisted launch rows when a
//! pane outlived the Delta process that was tracking it.
//!
//! The running set ([`RunningSubagent`]) is in-memory runtime state, filled
//! when the transcript fold sees a launch line. A BACKGROUND subagent keeps
//! running in a pane that survived a restart of Delta, but the re-adoption's
//! catch-up resumes from the stored cursor, which is already past that launch
//! line, so without this the set stays empty: the sends envelope reports no
//! running subagent and the navigator spinner and the conversation indicator
//! stay dark until the completion notification arrives. The launch row is what
//! survives the restart, and the fold marks every row it lit the indicator for
//! ([`SubagentLaunchStore::record_subagent_indicator`]), so a re-adoption lights
//! exactly those again — never a background `Bash`, which never lit it.
//!
//! A resume is the opposite case: `claude --resume` is a new process, so every
//! background subagent the previous one launched is gone and its completion can
//! never arrive. Its rows would otherwise be restored the next time a pane of
//! this session is re-adopted, and stay lit until the session ends, so the
//! resume drops them.
//!
//! [`RunningSubagent`]: crate::interactor::session_actor::runtime::RunningSubagent
//! [`SubagentLaunchStore::record_subagent_indicator`]: crate::ports::SubagentLaunchStore::record_subagent_indicator

use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::RunningSubagent;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Light the running-subagent indicator again for every background
    /// subagent a re-adopted pane may still be running: one entry per
    /// outstanding launch row that lit it, with the launching thread, ids and
    /// display fields the row kept, and a `SubagentStarted` on the async seam
    /// for each entry newly added, so a viewer already connected lights it too.
    ///
    /// Call it after the re-adoption's `TurnInput::Close` (whose sweep keeps
    /// background entries, and every restored entry is one) and before its
    /// catch-up sync: a completion the agent wrote while Delta was down is then
    /// folded against the restored entry and finishes it, and the row it
    /// clears is not read back afterwards.
    ///
    /// A store error is logged rather than returned: the pane is adopted either
    /// way, and the cost is the dark indicator this exists to fix.
    pub(in crate::interactor) async fn restore_running_subagents(&mut self) {
        let launches = match self.store.running_subagent_launches(self.id).await {
            Ok(launches) => launches,
            Err(err) => {
                tracing::warn!(
                    session_id = %self.id,
                    error = %err,
                    "reading the outstanding subagent launches of a re-adopted session \
                     failed; its running subagents stay dark until they complete"
                );
                return;
            }
        };
        for launch in launches {
            let newly = self.state.start_subagent(RunningSubagent {
                thread_id: launch.thread_id,
                tool_use_id: launch.tool_use_id.clone(),
                task_id: launch.task_id,
                subagent_type: launch.subagent_type.clone(),
                description: launch.description.clone(),
                background: true,
            });
            if !newly {
                continue;
            }
            tracing::info!(
                session_id = %self.id,
                tool_use_id = %launch.tool_use_id,
                "restored a background subagent the re-adopted pane may still be running"
            );
            self.emit_async_event(SessionEvent::SubagentStarted {
                session_id: self.id.clone(),
                thread_id: launch.thread_id,
                tool_use_id: launch.tool_use_id,
                subagent_type: launch.subagent_type,
                description: launch.description,
                background: true,
            });
        }
    }

    /// Drop the launch rows a previous `claude` process of this session left
    /// lit, now that a resume has replaced that process: its background
    /// subagents died with it, so a re-adoption must not restore them.
    ///
    /// Call it after the resume's catch-up sync, so a completion that process
    /// wrote before it ended still finds its launch row. A row whose entry the
    /// catch-up itself just lit is left alone: that entry is in the running set
    /// and goes the way every running entry does.
    ///
    /// A store error is logged rather than returned: the resume has happened,
    /// and the cost is a stale entry a later re-adoption lights until the
    /// session is closed.
    pub(in crate::interactor) async fn forget_subagents_of_the_previous_process(&mut self) {
        let launches = match self.store.running_subagent_launches(self.id).await {
            Ok(launches) => launches,
            Err(err) => {
                tracing::warn!(
                    session_id = %self.id,
                    error = %err,
                    "reading the subagent launches a resumed session's previous process \
                     left behind failed; a later re-adoption may light them"
                );
                return;
            }
        };
        for launch in launches {
            if self.state.is_running_subagent(&launch.tool_use_id) {
                continue;
            }
            if let Err(err) = self
                .store
                .clear_subagent_launch(self.id, &launch.tool_use_id)
                .await
            {
                tracing::warn!(
                    session_id = %self.id,
                    tool_use_id = %launch.tool_use_id,
                    error = %err,
                    "clearing a subagent launch the resumed session's previous process \
                     left behind failed; a later re-adoption may light it"
                );
            }
        }
    }
}
