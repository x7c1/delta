use crate::error::Result;
use crate::interactor::hooks::{is_subagent_tool, ASK_USER_QUESTION};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::PendingQuestion;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Handle a `PreToolUse` hook: RECORD the permission request, and — for an
    /// `Agent`/`Task` tool — trigger a transcript sync so the running-subagent
    /// indicator lights up without waiting for the next ambient sync.
    ///
    /// This hook fires for every tool call (including auto-approved and
    /// long-running ones), so it is not a reliable signal that a human answer
    /// is pending — recording the request here carries the `tool_use_id` needed
    /// to resolve it once the matching `tool_result` is later ingested. The
    /// browser notice is emitted by the `PermissionRequest` hook instead, which
    /// fires only for genuine interactive prompts (and owns its own row plus
    /// the Allow/Deny wait — see `on_permission_request`). PreToolUse itself
    /// never returns allow/deny. Routed through the session's mailbox so the
    /// record is ordered before any ingest that could resolve it.
    ///
    /// The running-subagent indicator is NO LONGER driven directly from this
    /// hook. The parent session's transcript ingest is now the source of truth
    /// — every `Agent`/`Task` `tool_use` block folded out of the PARENT's JSONL
    /// emits an `Effect::SubagentIndicatorStarted` (see `attribute_lines` /
    /// `sync_transcript`), which is what calls `start_subagent` and broadcasts
    /// `SubagentStarted`. A NESTED subagent's `Agent`/`Task` `tool_use` is
    /// written to the subagent's own JSONL — never the parent's — so the
    /// parent's ingest naturally skips it, and a stuck indicator on the parent
    /// is structurally impossible. The older PreToolUse-driven mechanism could
    /// not distinguish parent from nested calls on real Claude Code (every
    /// hook field — `session_id`, `transcript_path`, `cwd`, `caller` — was
    /// presented as the parent for both), which is what made depth>=2 subagent
    /// trees leave the parent indicator lit forever.
    ///
    /// To keep the indicator latency tight, this hook runs `sync_transcript`
    /// when the tool is `Agent`/`Task`: the assistant message carrying the
    /// `tool_use` block has already been flushed to the parent's JSONL before
    /// `PreToolUse` fires, so the sync sees it immediately. Any events the
    /// sync returns (notably `SubagentStarted`) are propagated to the caller
    /// for broadcast. The same sync also pre-populates the in-memory entry for
    /// a foreground subagent so the matching `PostToolUse` can clear it.
    ///
    /// The one exception is Claude Code's built-in [`ASK_USER_QUESTION`] tool:
    /// it presents a multiple-choice question rather than a gateable action, so
    /// here — where the recorded row carries the `tool_use_id` that the later
    /// `tool_result` resolves it by — Delta also remembers the question and
    /// emits [`SessionEvent::QuestionAsked`] so the browser shows a dedicated
    /// question card. (Its sibling `PermissionRequest` hook passes straight
    /// through; see `on_permission_request`.)
    pub(in crate::interactor) async fn on_pre_tool_use(
        &mut self,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: &str,
        transcript_path: &str,
    ) -> Result<Vec<SessionEvent>> {
        // A nested subagent's tool call carries the PARENT session's
        // `session_id` (Claude Code dispatches hooks that way) but its
        // `transcript_path` points at the subagent's own JSONL. Empirically
        // (CC 2.1.193) this is not reliable enough on its own to filter a
        // nested `Agent`/`Task` launch — a depth=2 nested launch's hook can
        // carry the PARENT's `transcript_path` — so the running-subagent
        // indicator is now driven from the parent transcript ingest itself
        // instead (see attribute_lines / sync_transcript). This guard still
        // serves to keep stray permission-request rows off the parent when a
        // nested call IS reliably tagged with its own transcript path (the
        // older payload shape).
        if self.is_foreign_transcript(transcript_path).await? {
            return Ok(vec![]);
        }

        let request = self
            .store
            .record_permission_request(self.id, tool_name, tool_input_json, Some(tool_use_id))
            .await?;

        if tool_name == ASK_USER_QUESTION {
            // Attribute the question to the in-progress turn's thread, resolved
            // the same way the streaming preview resolves its thread (see
            // `on_message_display`): the in-flight send's thread, else the
            // latest persisted user message, else the session's main thread.
            // AskUserQuestion blocks synchronously within the turn, so this IS
            // the asking thread — the browser shows the card only there. The
            // in-flight-send step is what keeps a mid-turn branch question on
            // the new branch thread, whose user line is not yet ingested.
            let thread_id = self.store.in_progress_turn_thread(self.id).await?;
            // Mirror the broadcast into queryable runtime state, so a client
            // that misses the event (socket down) rebuilds the question card
            // from the sends envelope. Cleared on resolution or turn end.
            self.state.set_pending_question(PendingQuestion {
                request_id: request.id,
                thread_id,
                tool_input_json: tool_input_json.to_owned(),
            });
            return Ok(vec![SessionEvent::QuestionAsked {
                session_id: self.id.clone(),
                request_id: request.id,
                thread_id,
                tool_input_json: tool_input_json.to_owned(),
            }]);
        }

        if is_subagent_tool(tool_name) {
            // Force a transcript sync now. The assistant message carrying the
            // `Agent`/`Task` `tool_use` block was flushed to the parent's
            // JSONL before this hook fired, so the sync sees it on this very
            // call, emits `Effect::SubagentIndicatorStarted`, and produces the
            // `SessionEvent::SubagentStarted` that lights the indicator — and
            // adds the in-memory entry the foreground `PostToolUse` clears.
            //
            // The sync is gated to subagent tools so an unrelated tool call
            // does not pay the read-from-disk cost on every `PreToolUse`; the
            // ambient tail covers the general case. A session row may not yet
            // be registered (e.g. a hook arriving before `SessionStart`), in
            // which case the sync is a no-op.
            if let Some(session) = self.store.session(self.id).await? {
                let (_messages, sync_events) = self.sync_transcript(&session).await?;
                return Ok(sync_events);
            }
        }

        Ok(vec![])
    }
}
