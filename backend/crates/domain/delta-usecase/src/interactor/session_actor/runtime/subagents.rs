//! [`RunningSubagent`]: the subagents (`Agent`/`Task` tool calls) currently
//! running in the session's turn, and their start/finish/upgrade tracking.

use delta_model::ThreadId;

use super::SessionRuntime;

/// A subagent (the `Agent`/`Task` tool) currently running inside the session's
/// main turn.
///
/// A subagent runs in its own transcript that Delta never tails, so the main
/// conversation pane shows nothing while it works. This is the queryable
/// counterpart of the `subagent_started`/`subagent_finished` broadcasts: those
/// events are lost for a client whose socket was down when they fired, so the
/// sends envelope reports the running set and a reconnecting client rebuilds
/// its indicator from a plain refetch — exactly like [`PendingPermission`] and
/// [`PendingQuestion`].
///
/// A foreground subagent's running window is the synchronous
/// `PreToolUse(Agent)` → `PostToolUse(Agent)` hook pair, correlated by
/// `tool_use_id`, cleared when the turn returns to idle — a foreground subagent
/// cannot outlive its turn.
///
/// A background subagent (`run_in_background: true`) outlives the launching
/// turn: its `PostToolUse` fires immediately at launch (the call returned, not
/// the subagent), and its real completion arrives much later as a
/// `<task-notification>` transcript line. So a background entry is NOT finished
/// by the immediate `PostToolUse` and is NOT swept at turn end; it is finished
/// only when the completion notification is folded (see
/// `Effect::SubagentCompleted`). The [`Self::background`] flag drives both
/// distinctions.
///
/// [`PendingPermission`]: super::PendingPermission
/// [`PendingQuestion`]: super::PendingQuestion
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningSubagent {
    /// The thread that launched the subagent, resolved (via
    /// `SessionStore::in_progress_turn_thread`) the same way the in-flight
    /// turn's thread is. A reconnecting client carries this so it can keep the
    /// launching thread's running indicator lit — and its unread badge
    /// suppressed — until the subagent finishes, which for a BACKGROUND
    /// subagent outlives the launching turn.
    pub thread_id: ThreadId,
    /// The `tool_use_id` of the `Agent`/`Task` call, the primary key that
    /// finishes it — the matching `PostToolUse` for a foreground entry, or the
    /// completion `<task-notification>` carrying this same id for a background
    /// entry.
    pub tool_use_id: String,
    /// The background-task identifier the launching tool's `tool_result`
    /// reports for a BACKGROUND subagent. Learned via the `PostToolUse(Agent)`
    /// hook (which reads `agentId` from the result content) and used as a
    /// fallback correlation key when matching a `<task-notification>` whose
    /// `<tool-use-id>` element was stripped from the user-message body — the
    /// notification's `<task-id>` element still routes back here. `None` until
    /// that hook has run, and stays `None` for foreground subagents (their
    /// `PostToolUse` finishes the entry directly, so the fallback key is
    /// never needed).
    pub task_id: Option<String>,
    /// The subagent type from the tool input (e.g. `general-purpose`), if the
    /// call carried one.
    pub subagent_type: Option<String>,
    /// The short task description from the tool input, if the call carried one,
    /// for display next to the indicator.
    pub description: Option<String>,
    /// Whether the launch carried `run_in_background: true`. A background
    /// subagent survives the immediate `PostToolUse` and the turn-end sweep; a
    /// foreground one is finished on its `PostToolUse` and swept at turn end.
    pub background: bool,
}

impl SessionRuntime {
    /// Remove and return ALL running-subagent entries, regardless of kind.
    ///
    /// This is the **process-gone sweep**, used by the two graceful signals
    /// that the session's `claude` process is confirmed gone —
    /// `on_session_end`'s normal-end path and `close_session`. Once the process
    /// is gone no more of this session's transcript is ingested, so a BACKGROUND
    /// entry's completion `<task-notification>` can never be folded and
    /// [`Self::finish_subagent`] can never fire for it: the indicator would
    /// otherwise stay lit forever. Draining hands every lingering entry back to
    /// the caller so it can emit a `SubagentFinished` per entry and clear the
    /// persisted launch row.
    ///
    /// How it differs from the other clears:
    /// - [`Self::finish_subagent`] removes ONE entry, driven by a single folded
    ///   completion notification — the normal, process-alive end.
    /// - [`Self::forget_turn`] also clears the whole set, but on session
    ///   DELETION and event-lessly (the persisted rows go by cascade). This
    ///   returns the drained entries precisely because the session still
    ///   exists, so the caller must emit events and drop persisted state itself.
    ///
    /// At both call sites the `TurnInput::Close` transition has already swept
    /// the foreground entries (see [`Self::apply_turn`]), so in practice this
    /// returns the surviving BACKGROUND entries. Draining the whole set anyway
    /// is deliberately kind-agnostic so nothing lingering can be missed.
    pub fn drain_running_subagents(&mut self) -> Vec<RunningSubagent> {
        std::mem::take(&mut self.running_subagents)
    }

    /// Record a subagent (`Agent`/`Task` tool call) as started, returning
    /// whether it was newly added.
    ///
    /// Keyed by `tool_use_id`: a duplicate `PreToolUse` for an already-tracked
    /// id is a no-op (returns `false`), so a retried hook delivery cannot list
    /// the same subagent twice. New entries are appended so the set stays in
    /// start order for display.
    pub fn start_subagent(&mut self, subagent: RunningSubagent) -> bool {
        if self
            .running_subagents
            .iter()
            .any(|s| s.tool_use_id == subagent.tool_use_id)
        {
            return false;
        }
        self.running_subagents.push(subagent);
        true
    }

    /// Drop the FOREGROUND running subagent with this `tool_use_id`, returning
    /// whether one was actually removed.
    ///
    /// This is the `PostToolUse(Agent)` path. It only removes a foreground
    /// entry: a background subagent's `PostToolUse` fires immediately at launch
    /// (the call returned, not the subagent), so it must NOT finish it — the
    /// completion `<task-notification>` does, via [`Self::finish_subagent`].
    ///
    /// Keyed so a `PostToolUse` for an unknown id, one already cleared at turn
    /// end, or a background id (still running) is a harmless no-op (returns
    /// `false`) rather than emitting a spurious "finished".
    pub fn finish_foreground_subagent(&mut self, tool_use_id: &str) -> bool {
        let before = self.running_subagents.len();
        self.running_subagents
            .retain(|s| s.tool_use_id != tool_use_id || s.background);
        self.running_subagents.len() != before
    }

    /// Drop the running subagent with this `tool_use_id` regardless of kind,
    /// returning whether one was actually removed.
    ///
    /// This is the background-completion path: when a completion
    /// `<task-notification>` is folded (`Effect::SubagentCompleted`), the
    /// background entry it correlates to by `tool_use_id` is removed here.
    ///
    /// Keyed and kind-agnostic so it tolerates an unknown id: a background
    /// `Bash` (`run_in_background: true`) also produces `SubagentCompleted`, but
    /// Delta never STARTS an indicator for `Bash`, so its id is untracked and
    /// this is a harmless no-op (returns `false`).
    pub fn finish_subagent(&mut self, tool_use_id: &str) -> bool {
        let before = self.running_subagents.len();
        self.running_subagents
            .retain(|s| s.tool_use_id != tool_use_id);
        self.running_subagents.len() != before
    }

    /// Attach a learned `task_id` to the running subagent with this
    /// `tool_use_id`, returning `true` when the entry's `task_id` actually
    /// changed (so the caller knows to persist the upgrade through the store).
    /// Upgrading an unknown id (or an entry already carrying a matching
    /// `task_id`) returns `false` — no row was changed, so nothing downstream
    /// needs to fire.
    ///
    /// This is the BACKGROUND subagent's `PostToolUse(Agent)` path: the hook
    /// reads `agentId` from the launching tool's `tool_result` and records it
    /// here so a subsequent `<task-notification>` whose `<tool-use-id>` element
    /// was stripped can still be matched by its `<task-id>` element.
    pub fn upgrade_subagent_task_id(&mut self, tool_use_id: &str, task_id: &str) -> bool {
        let Some(entry) = self
            .running_subagents
            .iter_mut()
            .find(|s| s.tool_use_id == tool_use_id)
        else {
            return false;
        };
        if entry.task_id.as_deref() == Some(task_id) {
            return false;
        }
        entry.task_id = Some(task_id.to_owned());
        true
    }

    /// The `task_id` the runtime knows for this `tool_use_id`, if any.
    ///
    /// Read by the sync path right after [`Effect::SubagentLaunched`] persists
    /// the launch row: a background subagent's immediate `PostToolUse(Agent)`
    /// usually fires before the launch line is folded, so the hook recorded
    /// the `agentId` on the runtime entry but could not yet persist it on the
    /// launch row (which did not exist). The sync flushes that pending upgrade
    /// here so the persisted row carries the fallback correlation key for the
    /// eventual `<task-notification>`.
    ///
    /// [`Effect::SubagentLaunched`]: delta_attribution::Effect::SubagentLaunched
    pub fn pending_subagent_task_id(&self, tool_use_id: &str) -> Option<&str> {
        self.running_subagents
            .iter()
            .find(|s| s.tool_use_id == tool_use_id)
            .and_then(|s| s.task_id.as_deref())
    }

    /// Record the `agentId` a `PostToolUse(Agent)` reported, so the next
    /// transcript sync can fold it into the running-subagent entry once that
    /// entry exists. Entry-or-insert: a retried hook delivery for the same
    /// `tool_use_id` does not overwrite the first observed value.
    ///
    /// See [`Self::pending_post_tool_use_agent_ids`] for the race this buffer
    /// covers.
    ///
    /// [`Self::pending_post_tool_use_agent_ids`]: SessionRuntime::pending_post_tool_use_agent_ids
    pub(in crate::interactor) fn record_pending_post_tool_use_agent_id(
        &mut self,
        tool_use_id: &str,
        agent_id: &str,
    ) {
        self.pending_post_tool_use_agent_ids
            .entry(tool_use_id.to_owned())
            .or_insert_with(|| agent_id.to_owned());
    }

    /// Take the buffered `agentId` for this `tool_use_id`, if any. Drained by
    /// the `Effect::SubagentIndicatorStarted` arm of `sync_transcript` once it
    /// creates the in-memory running entry — after which the value, if present,
    /// is applied to the entry and persisted on the launch row.
    pub(in crate::interactor) fn drain_pending_post_tool_use_agent_id(
        &mut self,
        tool_use_id: &str,
    ) -> Option<String> {
        self.pending_post_tool_use_agent_ids.remove(tool_use_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subagent(tool_use_id: &str, background: bool) -> RunningSubagent {
        RunningSubagent {
            thread_id: ThreadId(1),
            tool_use_id: tool_use_id.to_owned(),
            task_id: None,
            subagent_type: None,
            description: None,
            background,
        }
    }

    #[test]
    fn drain_running_subagents_returns_every_entry_and_empties_the_set() {
        let mut runtime = SessionRuntime::default();
        // A foreground and a background entry, so the drain is proven
        // kind-agnostic (unlike the turn-end sweep, which keeps background).
        runtime.start_subagent(subagent("toolu_fg", false));
        runtime.start_subagent(subagent("toolu_bg", true));

        let drained = runtime.drain_running_subagents();

        assert_eq!(
            drained
                .iter()
                .map(|s| s.tool_use_id.clone())
                .collect::<Vec<_>>(),
            vec!["toolu_fg".to_owned(), "toolu_bg".to_owned()],
            "drain returns all entries in start order, regardless of kind"
        );
        assert!(
            runtime.live_state().running_subagents.is_empty(),
            "drain leaves the running set empty"
        );
    }

    #[test]
    fn drain_running_subagents_is_empty_when_none_are_running() {
        let mut runtime = SessionRuntime::default();

        assert!(
            runtime.drain_running_subagents().is_empty(),
            "draining an empty set yields nothing"
        );
        assert!(runtime.live_state().running_subagents.is_empty());
    }
}
