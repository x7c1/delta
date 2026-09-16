use std::time::Instant;

use crate::error::Result;
use crate::interactor::lifecycle::UnboundLaunchEnd;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

/// How many of a reaped launch's last pane lines are kept as its failure reason.
///
/// Enough to carry a prompt and the question above it — the shapes that actually
/// stall a launch — without pasting a whole screen of TUI frame into a row that
/// is read as prose on the failed session's screen.
const CAPTURED_PANE_LINES: usize = 12;

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Reap this session's launch if it never became ready before its deadline
    /// (the watchdog sweep), covering both a fresh spawn and a resume.
    ///
    /// Two fire-and-forget launch shapes can stall the UI on "pending" forever:
    ///
    /// - **Fresh spawn**: `claude` is launched in a tmux pane and the only thing
    ///   that registers/binds it is its first `UserPromptSubmit` (or
    ///   `SessionStart`) hook. If it crashes, exits, or hangs on auth before that
    ///   hook fires, nothing else would time the dangling spawn out. The sweep
    ///   removes an unbound spawn whose deadline has elapsed — unless a PTY
    ///   bridge is attached to its pane, in which case the pane is not something
    ///   nobody can reach: somebody is looking at it, and quite possibly
    ///   answering the prompt the launch stopped on
    ///   (`SessionRuntime::take_stale_pending`).
    /// - **Resumed session**: `claude --resume <id>` binds the pane immediately
    ///   but the first prompt is held until `SessionStart(source=resume)` signals
    ///   readiness. A resume that never becomes ready (the resume crashes/hangs,
    ///   or transcript replay fails after the existence gate) leaves that held
    ///   prompt parked forever — and a resume records no pending spawn, so the
    ///   spawn sweep above does not cover it. The sweep removes a resuming entry
    ///   whose readiness deadline has elapsed, cancelling its held first prompt.
    ///
    /// A third shape — a session that has only been *accepted*, with its launch
    /// preparation still running — is deliberately outside this sweep: it holds
    /// a `LaunchingSpawn`, not a pending one, so neither drain sees it. It has
    /// no pane to kill, and its bind deadline only starts when the preparation
    /// checks in (`LaunchPrepared`) a beat before the pane is created; a slow
    /// `git fetch` must not eat that deadline. Its backstop is the launch task's
    /// own [`LAUNCH_PREP_DEADLINE`], after which the launch fails itself and
    /// reports the same `SpawnFailed` (with a `reason`).
    ///
    /// An **adapter-backed** (Codex) session is only ever that third shape while
    /// it is starting: its launch is accepted and deferred exactly like a
    /// Claude one, but the bind is the launch's own last step rather than
    /// something a hook has to deliver afterwards, so it never becomes a pending
    /// spawn and `pending_spawn_deadline` never applies to it. The launch
    /// preparation deadline — which covers the worktree build, the `connect` and
    /// the `thread/start` together — is its only watchdog, and it needs no
    /// other: an adapter launch that hangs hangs *inside* that window.
    ///
    /// For each stale launch it kills the tmux pane (best-effort, guarded by
    /// `has_session`) and produces a [`SessionEvent::SpawnFailed`] so the browser
    /// can surface the failure and clear the optimistic pending chip. For a
    /// stale *spawn* that event carries a reason built by
    /// [`InteractorCore::deadline_reason`] — the deadline it missed, and what
    /// its pane was showing, read a moment before the kill. The same
    /// `SpawnFailed` shape is reused for both: it already carries the
    /// `session_id` + `pane_token` the browser needs, and a resume failure is the
    /// same "this launch never came up" outcome from the UI's point of view, so a
    /// sibling event would add a wire variant without adding information.
    ///
    /// `now` is injected (rather than read here) so the watchdog is deterministic
    /// under test. The usecase returns the events to broadcast — the server owns
    /// the periodic tick that fans this out and broadcasts the result.
    ///
    /// [`LAUNCH_PREP_DEADLINE`]: crate::launch_config::LAUNCH_PREP_DEADLINE
    pub(in crate::interactor) async fn reap_stale_launch(
        &mut self,
        now: Instant,
    ) -> Result<Vec<SessionEvent>> {
        let stale_spawn = self
            .state
            .take_stale_pending(now, self.launch.pending_spawn_deadline);
        let stale_resume = self
            .state
            .take_stale_resuming(now, self.launch.resume_ready_deadline);

        let mut events = Vec::new();
        if let Some(spawn) = stale_spawn {
            tracing::warn!(
                token = %spawn.token.as_str(),
                session_id = %self.id,
                "reaping a spawn that never bound before its deadline; \
                 killing its pane and reporting SpawnFailed"
            );
            // Read the pane BEFORE the cleanup kills it: it is the only witness
            // to why this launch went quiet, and in a moment it is gone.
            let reason = self
                .deadline_reason(&spawn.pane, self.launch.pending_spawn_deadline)
                .await;
            // The shared cleanup (`cancel_unbound_launch`).
            events.push(
                self.cancel_unbound_launch(
                    Some(&spawn.token),
                    UnboundLaunchEnd::Failed(Some(reason)),
                )
                .await,
            );
        }
        if let Some(resuming) = stale_resume {
            tracing::warn!(
                token = %resuming.token.as_str(),
                session_id = %self.id,
                had_held_prompt = resuming.held_prompt.is_some(),
                "reaping a resume that never became ready before its deadline; \
                 killing its pane, cancelling any held prompt, reporting SpawnFailed"
            );
            self.kill_pane_best_effort(resuming.token.as_str()).await;
            // The session's pane is gone: feed `Close` into the turn machine,
            // which cancels the held first prompt's outstanding send (if any)
            // so its row does not shadow correlation when the session is later
            // resumed again.
            let _ = self.apply_turn_input(crate::turn::TurnInput::Close).await;
            events.push(SessionEvent::SpawnFailed {
                session_id: self.id.clone(),
                pane_token: Some(resuming.token.as_str().to_owned()),
                reason: None,
                cancelled: false,
            });
        }
        Ok(events)
    }
}

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// What to record as the failure reason for a launch the watchdog gave up
    /// on: the deadline it missed, plus what its pane was showing if the pane
    /// can still be read.
    ///
    /// The watchdog observes silence, so it cannot name a cause. But it knows
    /// the launch did not bind before its deadline, which is worth saying on its
    /// own, and the pane usually says the rest — an authentication prompt,
    /// Claude Code's workspace-trust dialog, a crash backtrace — because
    /// whatever stopped the launch is still on screen.
    ///
    /// Best-effort about the capture only: a pane that has already died, or a
    /// `capture-pane` that fails, yields the deadline sentence on its own rather
    /// than failing the reap. Must be called before the pane is killed.
    async fn deadline_reason(&self, pane: &str, deadline: std::time::Duration) -> String {
        // Rounded UP to a whole second, so a deadline shortened to milliseconds
        // under test still reads as a sentence rather than as "within 0", and a
        // fractional one is never understated as the whole second below it —
        // the sentence is shown to the user as what the launch was given.
        let seconds = (deadline.as_millis().div_ceil(1_000) as u64).max(1);
        let headline = format!(
            "The launch did not start within {seconds} second{}, so Delta gave it up.",
            if seconds == 1 { "" } else { "s" },
        );
        match self.capture_pane_tail(pane).await {
            Some(tail) => format!("{headline}\n\nIts terminal was showing:\n{tail}"),
            None => headline,
        }
    }

    /// The last few non-blank lines of `pane`, or `None` when there is nothing
    /// readable there.
    ///
    /// Trimmed to [`CAPTURED_PANE_LINES`] because the whole point is the tail —
    /// a TUI's visible screen is mostly frame and blank filler, and what stopped
    /// the launch is whatever it printed last. Blank lines are dropped for the
    /// same reason (a captured screen is padded to its full height), and the
    /// result is only returned when something is left.
    async fn capture_pane_tail(&self, pane: &str) -> Option<String> {
        let captured = match self.tmux.capture_pane(pane).await {
            Ok(captured) => captured,
            Err(err) => {
                tracing::warn!(
                    pane = %pane,
                    error = %err,
                    "could not capture the pane of a launch being reaped; \
                     reporting the deadline alone"
                );
                return None;
            }
        };
        let lines: Vec<&str> = captured
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
            .collect();
        let tail = lines
            .iter()
            .skip(lines.len().saturating_sub(CAPTURED_PANE_LINES))
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        (!tail.is_empty()).then_some(tail)
    }

    /// Best-effort pane teardown shared by every path that gives a launch up,
    /// however it ended: probe with `has_session` and kill if present,
    /// never letting a teardown error mask the failure report (the launch is
    /// already removed from the runtime state, so the failure event must still
    /// fire).
    pub(in crate::interactor) async fn kill_pane_best_effort(&self, token: &str) {
        match self.tmux.has_session(token).await {
            Ok(true) => {
                if let Err(err) = self.tmux.kill_session(token).await {
                    tracing::warn!(
                        token = %token,
                        error = %err,
                        "failed to kill the failed launch's pane (continuing)"
                    );
                }
            }
            Ok(false) => {}
            Err(err) => {
                tracing::warn!(
                    token = %token,
                    error = %err,
                    "failed to probe the failed launch's pane (continuing)"
                );
            }
        }
    }
}
