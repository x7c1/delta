//! [`AttributionState`]: the state threaded from line to line and batch to batch.

use std::collections::{BTreeMap, HashSet, VecDeque};

use delta_model::{PromptId, ThreadId};

use super::{OutstandingSend, SubagentLaunch};

/// The state the fold threads from line to line (and the caller threads from
/// batch to batch). Seeding it from the store and folding a batch is exactly
/// equivalent to folding the same lines in any other batching: that is the
/// replay invariant the corpus tests pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionState {
    /// The thread of the turn in progress: the thread of the most recent user
    /// line, advanced by matched sends and reset to `main` by external input.
    /// Seeded from the latest persisted user message (defaulting to `main`).
    pub carry_thread: ThreadId,
    /// The outstanding `dispatched` sends in dispatch (FIFO) order. Only the
    /// head is ever correlated against — mirroring the store's
    /// `head_dispatched_send`, which always returns the oldest `dispatched`
    /// row — and the next human line consumes it, exposing the next send.
    ///
    /// Under the single-outstanding dispatch rule a live session seeds at most
    /// one element here. The queue form is what makes whole-history replay
    /// work: seeding every send of a session in dispatch order folds the full
    /// transcript in one pass, each echo consuming its send in turn.
    pub outstanding: VecDeque<OutstandingSend>,
    /// The outstanding background task launches, keyed by the launching
    /// tool_use `id` (the `toolu_...` value). A background subagent (an
    /// async-by-default `Agent`/`Task`) or a background Bash
    /// (`run_in_background: true`) returns immediately and its completion is
    /// injected later as a `<task-notification>` user line carrying that same
    /// id in its `<tool-use-id>` element. Looking the id up here attributes
    /// the notification (and the assistant continuation it drives) to the
    /// thread that LAUNCHED the task, instead of blindly inheriting whatever
    /// thread is current when it lands — which is wrong whenever the user
    /// moved threads while the task ran.
    ///
    /// Each entry also carries a `task_id` learned later (via the
    /// `PostToolUse(Agent)` hook reading `agentId` out of the launching tool's
    /// `tool_result`), used as a fallback correlation key when Claude Code's
    /// notification body drops the `<tool-use-id>` element — only the
    /// `<task-id>` survives in that case, and matching by it still routes the
    /// completion to the launching thread.
    ///
    /// A map (not a single head) because several background tasks — launched
    /// from different threads, possibly nested — can be outstanding at once,
    /// and a completion must find its own launch by id. Like `outstanding`,
    /// this survives across sync windows by being seeded from a persisted
    /// store at batch start and mutated through effects: a launch is recorded
    /// ([`Effect::SubagentLaunched`]) when first seen and cleared
    /// ([`Effect::SubagentCompleted`]) when its notification is folded.
    /// `BTreeMap` keeps the seed-from-store ↔ fold round-trip deterministic.
    pub launched_threads: BTreeMap<String, SubagentLaunch>,
    /// The `promptId`s of the slash/local-command groups seen in this fold. A
    /// local command (e.g. `/review-pr`) is recorded as several `type: "user"`
    /// lines sharing one `promptId`: a leading `<local-command-caveat>` (the
    /// only one Claude flags `isMeta`), the bare command-name line, then the
    /// command's `<local-command-stdout>`/`<local-command-stderr>` output.
    /// Recording the caveat's `promptId` here lets the later same-`promptId`
    /// lines be recognized as command machinery (folded to [`Role::Meta`]) and
    /// the command-name line, when it equals an outstanding send, be resolved as
    /// a degenerate completed turn — a local command fires no `UserPromptSubmit`
    /// echo and no `Stop`, so without this its dispatched send would wedge the
    /// turn machine in `AwaitingEcho` forever.
    ///
    /// Threaded through the fold state (like `launched_threads`) so a batch cut
    /// between the caveat and its trailing lines still groups them. It is NOT
    /// seeded from a persisted store: Claude writes a local-command group as one
    /// atomic transcript append (the lines share a timestamp), so the whole
    /// group always lands in a single tail batch in production; whole-history
    /// replay sees the caveat before its members within the one pass. A
    /// `HashSet` is fine: it is only ever membership-tested (never iterated for
    /// output), and its `PartialEq` is order-independent, so the threaded-state
    /// equality the batch-split replay property pins still holds.
    pub local_command_prompts: HashSet<PromptId>,
}

impl AttributionState {
    /// Seed a batch: the carry thread plus the at-most-one outstanding send.
    /// The launch map starts empty; use [`Self::with_launches`] to seed it from
    /// the persisted background-launch store.
    pub fn new(carry_thread: ThreadId, outstanding: Option<OutstandingSend>) -> Self {
        Self {
            carry_thread,
            outstanding: outstanding.into_iter().collect(),
            launched_threads: BTreeMap::new(),
            local_command_prompts: HashSet::new(),
        }
    }

    /// Seed a batch with the outstanding background-launch map alongside the
    /// carry thread and outstanding send. The map carries `(tool_use_id ->
    /// SubagentLaunch)` — the launching thread plus the optional `task_id`
    /// learned later — for every background task still awaiting its
    /// `<task-notification>`.
    pub fn with_launches(
        carry_thread: ThreadId,
        outstanding: Option<OutstandingSend>,
        launched_threads: BTreeMap<String, SubagentLaunch>,
    ) -> Self {
        Self {
            launched_threads,
            ..Self::new(carry_thread, outstanding)
        }
    }
}
