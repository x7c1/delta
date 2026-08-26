import type {
  CommsDirection,
  CommsFrame,
  CommsFrameKind,
  LaunchOption,
  Message,
  PendingPermission,
  PendingQuestion,
  PromptTemplate,
  ProviderAvailability,
  RunningSubagent,
  Send,
  Session,
  Thread,
} from '@delta/wire-gen';

/**
 * A small but representative multi-session seed so the UI is fully developable
 * with no backend. Two sessions in distinct lifecycle states exercise the
 * session list, the open/closed indicator, focusing a closed (view-only)
 * session, and the per-session thread tree:
 *
 * - `sess-mock-1` — **open**. A trunk `main` thread with a multi-turn
 *   conversation (a tool call and a thinking block to exercise the collapsible
 *   blocks) plus one child branch thread sprouting from an assistant message.
 * - `sess-mock-2` — **closed**. A separate session with its own `main` thread,
 *   a short transcript, and one child branch thread. It is on page 1 but is not
 *   the auto-focused session (the open `sess-mock-1` is), so it exercises a
 *   *non-focused* session showing its sub-thread tree expanded by default.
 * - `sess-mock-3` — **closed and resume-unavailable**. A readable session whose
 *   transcript is gone, so the backend would refuse to resume it. It is flagged
 *   `resumable: false`, which makes the mock `open`/`sends` handlers answer with
 *   `409 resume_unavailable` — exactly as the real server does — so the
 *   "this session cannot be resumed" UI is developable with no backend. It sorts
 *   just after the two detailed sessions (top of page 2), leaving page 1 and the
 *   auto-focus unchanged.
 * - `sess-mock-4` — **closed, Codex provider**. The other three run on Claude;
 *   this one carries `provider: 'codex'` so the navigator's provider badge is
 *   exercisable for both providers with no backend. It has an empty main thread
 *   (no messages) and sorts just below `sess-mock-3` (still on page 2), again
 *   leaving page 1 and the auto-focus unchanged.
 */

export const SESSION_ID = 'sess-mock-1';
export const SESSION_ID_2 = 'sess-mock-2';
export const SESSION_ID_3 = 'sess-mock-3';
export const SESSION_ID_4 = 'sess-mock-4';
export const MAIN_THREAD_ID = 1;
export const BRANCH_THREAD_ID = 2;
export const SESSION_2_MAIN_THREAD_ID = 3;
export const SESSION_2_BRANCH_THREAD_ID = 4;
export const SESSION_3_MAIN_THREAD_ID = 5;
export const SESSION_4_MAIN_THREAD_ID = 6;

/**
 * Number of sessions returned per page by the mock `GET /api/sessions`. Small on
 * purpose: with more seeded sessions than this, the list spans multiple pages so
 * the infinite-scroll path (a non-null `next_cursor`, then a final `null`) is
 * exercised in dev and e2e.
 */
export const SESSIONS_PAGE_SIZE = 2;

/**
 * Extra "filler" sessions beyond the two detailed ones above. They have a single
 * empty main thread (no messages) so they are cheap to seed; their only job is
 * to push the session list past one page. Each is older than the two detailed
 * sessions so it sorts after them (most-recently-active first), keeping the two
 * fully-featured sessions on page 1.
 *
 * The count is deliberately larger than fits in a single navigator viewport so
 * the windowed (virtualized) list keeps far fewer rows in the DOM than the full
 * list — the e2e asserts this bound. It also spans many pages, exercising the
 * cursor-paginated load path repeatedly.
 */
export const FILLER_SESSION_COUNT = 40;
const FIRST_FILLER_THREAD_ID = 100;

/** Total seeded sessions: the four detailed ones plus the filler. */
export const TOTAL_SEEDED_SESSIONS = 4 + FILLER_SESSION_COUNT;

export const mockSession: Session = {
  id: SESSION_ID,
  cwd: '/home/dev/projects/delta',
  transcript_path: '/tmp/transcript.jsonl',
  title: null,
  status: 'active',
  created_at: '2026-01-01T00:00:00Z',
  branch_at_launch: 'main',
  repo_root: '/home/dev/projects/delta',
  repository_display_name: 'dev/delta',
  provider: 'claude',
  provider_session_id: null,
  provider_thread_id: null,
};

export const mockSession2: Session = {
  id: SESSION_ID_2,
  cwd: '/home/dev/projects/scratch',
  transcript_path: '/tmp/transcript-2.jsonl',
  title: 'scratch notes',
  status: 'ended',
  created_at: '2026-01-02T00:00:00Z',
  branch_at_launch: 'feat/scratch-ideas',
  repo_root: '/home/dev/projects/scratch',
  repository_display_name: 'dev/scratch',
  provider: 'claude',
  provider_session_id: null,
  provider_thread_id: null,
};

export const mockSession3: Session = {
  id: SESSION_ID_3,
  cwd: '/home/dev/projects/old-experiment',
  transcript_path: '/tmp/transcript-3-gone.jsonl',
  title: 'resume-unavailable demo',
  status: 'ended',
  created_at: '2025-12-31T00:00:00Z',
  branch_at_launch: 'main',
  repo_root: '/home/dev/projects/old-experiment',
  // `null` exercises the legacy/non-git fallback path on the navigator
  // (renders the cwd basename instead of an `org/repo` label).
  repository_display_name: null,
  provider: 'claude',
  provider_session_id: null,
  provider_thread_id: null,
};

export const mockSession4: Session = {
  id: SESSION_ID_4,
  cwd: '/home/dev/projects/codex-lab',
  transcript_path: '/tmp/transcript-4.jsonl',
  title: 'codex refactor',
  status: 'ended',
  // Sorts just below sess-mock-3 (2025-12-31) yet above every filler (which
  // start at 2025-12-30T00:00:00Z and go older), so this Codex session lands at
  // the top-of-page-2 region and page 1 / the auto-focus stay unchanged.
  created_at: '2025-12-30T12:00:00Z',
  branch_at_launch: 'feat/codex-adapter',
  repo_root: '/home/dev/projects/codex-lab',
  repository_display_name: 'dev/codex-lab',
  // The one non-Claude seed: exercises the navigator provider badge's Codex
  // path (the other three sessions run on Claude). Its repository name is kept
  // distinct from the Claude seeds so text-based session-node locators in the
  // e2e specs (e.g. filter by `dev/delta`) still resolve to a single card.
  provider: 'codex',
  provider_session_id: null,
  provider_thread_id: null,
};

export const mockThreads: Thread[] = [
  {
    id: MAIN_THREAD_ID,
    session_id: SESSION_ID,
    title: 'main',
    parent_thread_id: null,
    root_message_uuid: null,
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: BRANCH_THREAD_ID,
    session_id: SESSION_ID,
    title: 'delta etymology',
    parent_thread_id: MAIN_THREAD_ID,
    root_message_uuid: 'uuid-a1',
    created_at: '2026-01-01T00:05:00Z',
  },
];

export const mockThreads2: Thread[] = [
  {
    id: SESSION_2_MAIN_THREAD_ID,
    session_id: SESSION_ID_2,
    title: 'main',
    parent_thread_id: null,
    root_message_uuid: null,
    created_at: '2026-01-02T00:00:00Z',
  },
  {
    id: SESSION_2_BRANCH_THREAD_ID,
    session_id: SESSION_ID_2,
    title: 'scratch ideas',
    parent_thread_id: SESSION_2_MAIN_THREAD_ID,
    root_message_uuid: 'uuid-s2-a1',
    created_at: '2026-01-02T00:05:00Z',
  },
];

export const mockThreads3: Thread[] = [
  {
    id: SESSION_3_MAIN_THREAD_ID,
    session_id: SESSION_ID_3,
    title: 'main',
    parent_thread_id: null,
    root_message_uuid: null,
    created_at: '2025-12-31T00:00:00Z',
  },
];

export const mockThreads4: Thread[] = [
  {
    id: SESSION_4_MAIN_THREAD_ID,
    session_id: SESSION_ID_4,
    title: 'main',
    parent_thread_id: null,
    root_message_uuid: null,
    created_at: '2025-12-30T12:00:00Z',
  },
];

export const mockMessagesByThread: Record<number, Message[]> = {
  [MAIN_THREAD_ID]: [
    {
      uuid: 'uuid-u1',
      session_id: SESSION_ID,
      thread_id: MAIN_THREAD_ID,
      role: 'user',
      linear_parent_uuid: null,
      semantic_parent_uuid: null,
      prompt_id: 'prompt-1',
      seq: 0,
      content_text: 'What is a delta?',
      content: [{ type: 'text', text: 'What is a delta?' }],
      created_at: '2026-01-01T00:00:01Z',
      model: null,
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: null,
      provider_item_id: null,
    },
    {
      uuid: 'uuid-a1',
      session_id: SESSION_ID,
      thread_id: MAIN_THREAD_ID,
      role: 'assistant',
      linear_parent_uuid: 'uuid-u1',
      semantic_parent_uuid: null,
      prompt_id: 'prompt-1',
      seq: 1,
      content_text:
        'A **delta** is the change between two states.\n\nIn math it is written `Δ`.',
      content: [
        { type: 'thinking', thinking: 'The user is asking for a definition.' },
        {
          type: 'text',
          text: 'A **delta** is the change between two states.\n\nIn math it is written `Δ`.',
        },
      ],
      created_at: '2026-01-01T00:00:02Z',
      model: 'claude-opus-4-8',
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: 9400,
      provider_item_id: null,
    },
    {
      uuid: 'uuid-u2',
      session_id: SESSION_ID,
      thread_id: MAIN_THREAD_ID,
      role: 'user',
      linear_parent_uuid: 'uuid-a1',
      semantic_parent_uuid: null,
      prompt_id: 'prompt-2',
      seq: 2,
      content_text: 'List the files here.',
      content: [{ type: 'text', text: 'List the files here.' }],
      created_at: '2026-01-01T00:01:00Z',
      model: null,
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: null,
      provider_item_id: null,
    },
    {
      uuid: 'uuid-a2',
      session_id: SESSION_ID,
      thread_id: MAIN_THREAD_ID,
      role: 'assistant',
      linear_parent_uuid: 'uuid-u2',
      semantic_parent_uuid: null,
      prompt_id: 'prompt-2',
      seq: 3,
      content_text: 'Here are the files.',
      content: [
        {
          type: 'tool_use',
          id: 't1',
          name: 'Bash',
          input: { command: 'ls' },
        },
        {
          type: 'tool_result',
          tool_use_id: 't1',
          content: 'README.md\nsrc\npackage.json',
          is_error: false,
        },
        { type: 'text', text: 'Here are the files.' },
      ],
      created_at: '2026-01-01T00:01:01Z',
      model: 'claude-opus-4-8',
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: 9400,
      provider_item_id: null,
    },
  ],
  [BRANCH_THREAD_ID]: [
    {
      uuid: 'uuid-b1',
      session_id: SESSION_ID,
      thread_id: BRANCH_THREAD_ID,
      role: 'user',
      linear_parent_uuid: 'uuid-a1',
      semantic_parent_uuid: 'uuid-a1',
      prompt_id: 'prompt-3',
      seq: 0,
      content_text: 'Where does the word delta come from?',
      content: [{ type: 'text', text: 'Where does the word delta come from?' }],
      created_at: '2026-01-01T00:05:01Z',
      model: null,
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: null,
      provider_item_id: null,
    },
    {
      uuid: 'uuid-b2',
      session_id: SESSION_ID,
      thread_id: BRANCH_THREAD_ID,
      role: 'assistant',
      linear_parent_uuid: 'uuid-b1',
      semantic_parent_uuid: null,
      prompt_id: 'prompt-3',
      seq: 1,
      content_text:
        'It comes from the Greek letter Δ, named for its triangular shape.',
      content: [
        {
          type: 'text',
          text: 'It comes from the Greek letter Δ, named for its triangular shape.',
        },
      ],
      created_at: '2026-01-01T00:05:02Z',
      model: 'claude-opus-4-8',
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: 9400,
      provider_item_id: null,
    },
    // A paired tool call / tool result split across two messages, as Claude's
    // transcript records them: the `tool_use` lives in an assistant message
    // (uuid-b3, which renders as a tool call) and its `tool_result` arrives in
    // the following `user` message (uuid-b4). Because that result is paired to
    // a visible call, uuid-b4 renders NOTHING in the transcript — yet it is a
    // `user` row, so it still gets a mark on the thread timeline. That
    // renders-nothing carrier is exactly the case that makes a timeline axis
    // click resolve to a uuid with no transcript article, so the cross-lane
    // jump's DOM-ready poll can never land and must time out. It gives the
    // timeline e2e (and dogfooding) a real tool-heavy lane to exercise the
    // playhead-follow guard against.
    {
      uuid: 'uuid-b3',
      session_id: SESSION_ID,
      thread_id: BRANCH_THREAD_ID,
      role: 'assistant',
      linear_parent_uuid: 'uuid-b2',
      semantic_parent_uuid: null,
      prompt_id: 'prompt-3',
      seq: 2,
      content_text: null,
      content: [
        {
          type: 'tool_use',
          id: 'tb1',
          name: 'Read',
          input: { path: 'etymology.md' },
        },
      ],
      created_at: '2026-01-01T00:05:03Z',
      model: 'claude-opus-4-8',
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: 9400,
      provider_item_id: null,
    },
    {
      // A large (prose) turn between the tool_use and its result carrier. Its
      // only job on the timeline is to break the run of consecutive auxiliary
      // (small) marks: without it, uuid-b3 and uuid-b4 would be adjacent small
      // dots and cluster into a single mark whose representative is the
      // rendering uuid-b3 — so the renders-nothing carrier uuid-b4 would have
      // no standalone, clickable dot. With this large mark between them, both
      // uuid-b3 and uuid-b4 render as lone dots.
      uuid: 'uuid-b3b',
      session_id: SESSION_ID,
      thread_id: BRANCH_THREAD_ID,
      role: 'assistant',
      linear_parent_uuid: 'uuid-b3',
      semantic_parent_uuid: null,
      prompt_id: 'prompt-3',
      seq: 3,
      content_text: 'The file confirms the Greek origin of the word.',
      content: [
        {
          type: 'text',
          text: 'The file confirms the Greek origin of the word.',
        },
      ],
      created_at: '2026-01-01T00:05:05Z',
      model: 'claude-opus-4-8',
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: 9400,
      provider_item_id: null,
    },
    {
      uuid: 'uuid-b4',
      session_id: SESSION_ID,
      thread_id: BRANCH_THREAD_ID,
      role: 'user',
      linear_parent_uuid: 'uuid-b3b',
      semantic_parent_uuid: null,
      prompt_id: 'prompt-3',
      seq: 4,
      content_text: null,
      content: [
        {
          type: 'tool_result',
          tool_use_id: 'tb1',
          content: 'delta: from Greek Δ',
          is_error: false,
        },
      ],
      created_at: '2026-01-01T00:30:00Z',
      model: null,
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: null,
      provider_item_id: null,
    },
  ],
  [SESSION_2_MAIN_THREAD_ID]: [
    {
      uuid: 'uuid-s2-u1',
      session_id: SESSION_ID_2,
      thread_id: SESSION_2_MAIN_THREAD_ID,
      role: 'user',
      linear_parent_uuid: null,
      semantic_parent_uuid: null,
      prompt_id: 'prompt-s2-1',
      seq: 0,
      content_text: 'Remind me what scratch is for.',
      content: [{ type: 'text', text: 'Remind me what scratch is for.' }],
      created_at: '2026-01-02T00:00:01Z',
      model: null,
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: null,
      provider_item_id: null,
    },
    {
      uuid: 'uuid-s2-a1',
      session_id: SESSION_ID_2,
      thread_id: SESSION_2_MAIN_THREAD_ID,
      role: 'assistant',
      linear_parent_uuid: 'uuid-s2-u1',
      semantic_parent_uuid: null,
      prompt_id: 'prompt-s2-1',
      seq: 1,
      content_text: 'It is a throwaway working directory for experiments.',
      content: [
        {
          type: 'text',
          text: 'It is a throwaway working directory for experiments.',
        },
      ],
      created_at: '2026-01-02T00:00:02Z',
      model: 'claude-opus-4-8',
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: 9400,
      provider_item_id: null,
    },
  ],
  [SESSION_2_BRANCH_THREAD_ID]: [
    {
      uuid: 'uuid-s2-b1',
      session_id: SESSION_ID_2,
      thread_id: SESSION_2_BRANCH_THREAD_ID,
      role: 'user',
      linear_parent_uuid: 'uuid-s2-a1',
      semantic_parent_uuid: 'uuid-s2-a1',
      prompt_id: 'prompt-s2-2',
      seq: 0,
      content_text: 'Jot down a few ideas for later.',
      content: [{ type: 'text', text: 'Jot down a few ideas for later.' }],
      created_at: '2026-01-02T00:05:01Z',
      model: null,
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: null,
      provider_item_id: null,
    },
  ],
  [SESSION_3_MAIN_THREAD_ID]: [
    {
      uuid: 'uuid-s3-u1',
      session_id: SESSION_ID_3,
      thread_id: SESSION_3_MAIN_THREAD_ID,
      role: 'user',
      linear_parent_uuid: null,
      semantic_parent_uuid: null,
      prompt_id: 'prompt-s3-1',
      seq: 0,
      content_text: 'Summarize what we did in this experiment.',
      content: [
        { type: 'text', text: 'Summarize what we did in this experiment.' },
      ],
      created_at: '2025-12-31T00:00:01Z',
      model: null,
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: null,
      provider_item_id: null,
    },
    {
      uuid: 'uuid-s3-a1',
      session_id: SESSION_ID_3,
      thread_id: SESSION_3_MAIN_THREAD_ID,
      role: 'assistant',
      linear_parent_uuid: 'uuid-s3-u1',
      semantic_parent_uuid: null,
      prompt_id: 'prompt-s3-1',
      seq: 1,
      content_text:
        'We prototyped the parser and benchmarked it. The history is still readable here, but this session can no longer be resumed.',
      content: [
        {
          type: 'text',
          text: 'We prototyped the parser and benchmarked it. The history is still readable here, but this session can no longer be resumed.',
        },
      ],
      created_at: '2025-12-31T00:00:02Z',
      model: 'claude-opus-4-8',
      git_branch: 'main',
      cwd: '/home/dev/repo',
      response_time_ms: 9400,
      provider_item_id: null,
    },
  ],
};

/**
 * Deterministic id of the `ordinal`-th session spawned by a mock new-session
 * send within one {@link seedData} store. The real server mints a UUID; the
 * mock keeps the id predictable so tests can address the spawn (e.g. emit a
 * `spawn_failed` event carrying the same id the `POST /api/sends` returned).
 */
export function mockSpawnSessionId(ordinal: number): string {
  return `sess-mock-spawn-${ordinal}`;
}

/** In-memory store shape shared by the MSW handlers within one render. */
export interface MockStore {
  /** Sessions keyed by id, each with its open flag, threads, and main thread. */
  sessions: {
    session: Session;
    open: boolean;
    mainThreadId: number;
    threads: Thread[];
    /**
     * Whether a closed session can be resumed. Absent means yes (the common
     * case). `false` models a session whose transcript is gone: the `open` and
     * `sends` handlers then answer `409 resume_unavailable`, mirroring the real
     * server's resume gate.
     */
    resumable?: boolean;
    /**
     * True for an eagerly-created new-session row whose spawn has not bound
     * yet. Mirrors the real server's `spawning` status: the row is listed and
     * addressable by id from the moment its first send is accepted, but it is
     * not open (no pane is bound to it) until a `session_registered` event
     * activates it — or a `spawn_failed` deletes it. A send aimed at it while
     * it is starting is refused with `409 session_spawning`, as on the server.
     */
    spawning?: boolean;
    /**
     * The permission dialogs awaiting an answer, oldest first, mirrored from the
     * scripted `permission_requested`/`permission_resolved` events so the sends
     * envelope reports the queue head and its depth the way the real server does
     * (see `applyEvent`). A queue, not a slot: a provider running tool calls in
     * parallel leaves several outstanding at once. Absent/empty means nothing is
     * pending.
     */
    pendingPermissions?: PendingPermission[];
    /**
     * The `AskUserQuestion` currently presenting its options, mirrored from the
     * scripted `question_asked`/`permission_resolved` events so the sends
     * envelope reports it the way the real server does (see `applyEvent`).
     * Absent means no question is pending.
     */
    pendingQuestion?: PendingQuestion;
    /**
     * The subagents (`Agent`/`Task` tool) currently running, mirrored from the
     * scripted `subagent_started`/`subagent_finished` events so the sends
     * envelope reports the running set the way the real server does (see
     * `applyEvent`). Absent/empty means none is running.
     */
    runningSubagents?: RunningSubagent[];
    /**
     * The turn currently in flight, mirrored from the scripted
     * `turn_started` event (cleared by `turn_completed`/`turn_interrupted`)
     * so the sends envelope reports `in_flight` for the whole running turn
     * the way the real server does (see `applyEvent`). Without it the
     * envelope would fall back to `idle` as soon as the send is `matched`,
     * and the app's authoritative turn re-seed would wipe the running flag
     * the event just set. Absent means no turn is in flight.
     */
    activeTurn?: { sendId: number; threadId: number | null };
  }[];
  messagesByThread: Record<number, Message[]>;
  sends: Send[];
  nextThreadId: number;
  nextSendId: number;
  /** Ordinal of the next mock spawn (see {@link mockSpawnSessionId}). */
  nextSpawnOrdinal: number;
  /**
   * Registered launch options (the settings-screen registry). Listed the way the
   * server returns them: the rows Delta ships first (ascending `id`, i.e.
   * declared-catalog order), then the user's own newest first (descending `id`).
   */
  launchOptions: LaunchOption[];
  /** Id assigned to the next created launch option. */
  nextLaunchOptionId: number;
  /**
   * Registered prompt templates, oldest first (the settings-screen registry the
   * composer inserts from). Global rather than provider-scoped: the text is
   * prose, so it reads the same on every provider.
   */
  promptTemplates: PromptTemplate[];
  /** Id assigned to the next created prompt template. */
  nextPromptTemplateId: number;
  /**
   * Registered clone roots, newest first (the settings-screen "Clone roots"
   * section). Each is a directory whose direct children every
   * `GET /api/repositories` call probes for git clones. Kept with `created_at`
   * so the mock can sort the list newest-first the way the real server does;
   * the wire form omits the timestamp, and the `GET` handler strips it before
   * serialising.
   */
  cloneRoots: { path: string; created_at: string }[];
  /**
   * Repositories cloned during this mock session, keyed `<owner>/<repo>`.
   *
   * The real server learns that a clone exists by looking at the filesystem;
   * the mock has none, so a `repository_clone_completed` event records the
   * clone here and both `GET /api/prs` and `GET /api/repositories` read it —
   * which is what makes a PR row's `has_local_clone` actually flip on the
   * refetch that event triggers.
   */
  clonedRepos: { key: string; path: string }[];
}

/**
 * Build the filler sessions that push the list past one page. Each has one empty
 * main thread and no messages, and a `created_at` older than the two detailed
 * sessions so it sorts after them.
 */
function buildFillerSessions(): MockStore['sessions'] {
  return Array.from({ length: FILLER_SESSION_COUNT }, (_, index) => {
    const ordinal = index + 1;
    const threadId = FIRST_FILLER_THREAD_ID + index;
    const sessionId = `sess-mock-fill-${ordinal}`;
    // Strictly descending timestamps (one day apart, starting just before the
    // detailed sessions) keep every filler older than them and give each a
    // distinct, deterministic recency for stable pagination — scaling to any
    // FILLER_SESSION_COUNT without colliding or running off the calendar.
    const createdAt = new Date(
      Date.UTC(2025, 11, 31) - (index + 1) * 24 * 60 * 60 * 1000,
    ).toISOString();
    return {
      session: {
        id: sessionId,
        cwd: `/work/fill-${ordinal}`,
        transcript_path: `/tmp/transcript-fill-${ordinal}.jsonl`,
        title: `archived session ${ordinal}`,
        status: 'ended' as const,
        created_at: createdAt,
        // Older filler sessions predate the spawn-time git snapshot; leaving
        // them null exercises the navigator's fallback (cwd basename for the
        // repo line, session label for the branch line).
        branch_at_launch: null,
        repo_root: null,
        repository_display_name: null,
        provider: 'claude',
        provider_session_id: null,
        provider_thread_id: null,
      },
      open: false,
      mainThreadId: threadId,
      threads: [
        {
          id: threadId,
          session_id: sessionId,
          title: 'main',
          parent_thread_id: null,
          root_message_uuid: null,
          created_at: createdAt,
        },
      ],
    };
  });
}

/** Build a fresh deep copy of the seed data so handlers can mutate freely. */
export function seedData(): MockStore {
  const filler = buildFillerSessions();
  return {
    sessions: [
      {
        session: structuredClone(mockSession),
        open: true,
        mainThreadId: MAIN_THREAD_ID,
        threads: structuredClone(mockThreads),
      },
      {
        session: structuredClone(mockSession2),
        open: false,
        mainThreadId: SESSION_2_MAIN_THREAD_ID,
        threads: structuredClone(mockThreads2),
      },
      {
        session: structuredClone(mockSession3),
        open: false,
        mainThreadId: SESSION_3_MAIN_THREAD_ID,
        threads: structuredClone(mockThreads3),
        resumable: false,
      },
      {
        session: structuredClone(mockSession4),
        open: false,
        mainThreadId: SESSION_4_MAIN_THREAD_ID,
        threads: structuredClone(mockThreads4),
      },
      ...structuredClone(filler),
    ],
    messagesByThread: structuredClone(mockMessagesByThread),
    sends: [],
    nextThreadId: FIRST_FILLER_THREAD_ID + FILLER_SESSION_COUNT,
    nextSendId: 1,
    nextSpawnOrdinal: 1,
    // Seeded options so the settings screen is developable with no backend:
    // two Claude flags (a valued flag and a valueless-value flag) plus one Codex
    // option so the per-provider picker filter is exercisable without a backend,
    // plus the two rows Delta ships that the UI has to treat differently — one
    // Claude and one Codex, the latter carrying a long JSON `config` value, which
    // is the row the copy-and-adapt flow is built around and the one that proves
    // a value wider than the row is still shown in full.
    //
    // In list order: shipped rows first (ascending id), then the user's own
    // newest first. The shipped ids are deliberately *higher* than the user
    // rows', so a list handler that merely sorted by id could not pass for the
    // real ordering by accident.
    launchOptions: [
      {
        id: 100,
        label: 'Opus',
        name: '--model',
        value: 'opus',
        default_enabled: false,
        created_at: '2026-01-01T00:00:00Z',
        provider: 'claude',
        builtin: true,
      },
      {
        id: 101,
        label: 'Config: reasoning summary',
        name: 'config',
        value:
          '{"model_reasoning_summary": "auto", "sandbox_workspace_write": {"writable_roots": ["/home/dev/repos/delta", "/home/dev/repos/scratch"]}}',
        default_enabled: false,
        created_at: '2026-01-01T00:00:00Z',
        provider: 'codex',
        builtin: true,
      },
      {
        id: 3,
        label: 'Codex model',
        name: 'model',
        value: 'gpt-5',
        default_enabled: false,
        created_at: '2026-01-03T00:00:00Z',
        provider: 'codex',
        builtin: false,
      },
      {
        id: 2,
        label: null,
        name: '--permission-mode',
        value: 'auto',
        default_enabled: false,
        created_at: '2026-01-02T00:00:00Z',
        provider: 'claude',
        builtin: false,
      },
      {
        id: 1,
        label: 'My plugins',
        name: '--plugin-dir',
        value: '/home/dev/plugins',
        default_enabled: true,
        created_at: '2026-01-01T00:00:00Z',
        provider: 'claude',
        builtin: false,
      },
    ],
    nextLaunchOptionId: 4,
    // Seeded templates so the settings screen and the composer's insert menu are
    // developable with no backend. Oldest first, as the server returns them. The
    // second is deliberately multi-paragraph and long (> 200 characters) so list
    // rendering is exercised against a realistic body rather than a one-liner.
    promptTemplates: [
      {
        id: 1,
        label: 'Merge when green',
        text: 'Once CI is green, merge the PR and then update the plan doc.',
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
      },
      {
        id: 2,
        label: 'Review checklist',
        text: [
          'Review the diff on this branch with a critic\'s eye.',
          '',
          'Check, in order:',
          '- correctness first: off-by-one errors, error paths that swallow a failure, and anything that can panic on user input;',
          '- then the tests: does every new branch have one, and does a failing assertion say what broke?',
          '',
          'Report what you found as a short list, worst first, and leave the code alone until I say so.',
        ].join('\n'),
        created_at: '2026-01-02T00:00:00Z',
        updated_at: '2026-01-03T00:00:00Z',
      },
    ],
    nextPromptTemplateId: 3,
    // Empty by default — the Settings dialog's "Clone roots" section renders
    // its zero-state. The store lives inside `createMockApi`, so a test that
    // needs a registered root creates it the way the UI does: through the
    // mock's own `POST /api/clone-roots` handler.
    cloneRoots: [],
    clonedRepos: [],
  };
}

/** The mock filesystem's `$HOME`, used when a workdir list omits `path`. */
export const MOCK_WORKDIR_HOME = '/home/dev';

/**
 * The version string the mock `GET /api/version` returns. Fixed rather than
 * derived from the workspace `package.json` so mock-mode e2e can assert on it
 * verbatim without importing build metadata into the mock package.
 */
export const MOCK_VERSION = 'v0.0.0-mock';

/**
 * A tiny static directory tree backing the workdir-picker mock. Each key is a
 * canonical directory; the value lists its immediate subdirectory names. A path
 * absent from this map is treated as "not a directory" (400).
 */
const MOCK_WORKDIR_TREE: Record<string, string[]> = {
  '/': ['home'],
  '/home': ['dev'],
  [MOCK_WORKDIR_HOME]: ['projects', 'scratch'],
  '/home/dev/projects': ['delta', 'website'],
  '/home/dev/projects/delta': ['backend', 'frontend'],
  '/home/dev/projects/delta/backend': [],
  '/home/dev/projects/delta/frontend': [],
  '/home/dev/projects/website': [],
  '/home/dev/scratch': [],
};

/** The canonical parent of a path in the mock tree, or `null` at the root. */
function workdirParent(path: string): string | null {
  if (path === '/') {
    return null;
  }
  const parent = path.slice(0, path.lastIndexOf('/'));
  return parent === '' ? '/' : parent;
}

/**
 * One level of the mock browse for `path`, or `null` when `path` is not a known
 * directory (mapped to a 400 by the handler). Entries are name-sorted, dirs
 * only, mirroring the real server.
 */
export function workdirListing(path: string): {
  path: string;
  parent: string | null;
  entries: { name: string; path: string }[];
} | null {
  const names = MOCK_WORKDIR_TREE[path];
  if (!names) {
    return null;
  }
  const entries = [...names]
    .sort((a, b) => a.localeCompare(b))
    .map((name) => ({
      name,
      path: path === '/' ? `/${name}` : `${path}/${name}`,
    }));
  return { path, parent: workdirParent(path), entries };
}

/** Recently-used working directories for the mock, most-recent first. */
export function recentWorkdirs(): {
  path: string;
  last_used_at: string | null;
}[] {
  return [
    { path: '/home/dev/projects/delta', last_used_at: '2026-01-03T00:00:00Z' },
    { path: '/home/dev/projects/website', last_used_at: '2026-01-02T00:00:00Z' },
    { path: '/home/dev/scratch', last_used_at: null },
  ];
}

/**
 * The mock git repository: `/home/dev/projects/delta` and everything under it
 * is treated as one repo (root `/home/dev/projects/delta`, default branch
 * `main`). Every other directory is non-git, so the worktree toggle's show/hide
 * and the branches endpoint's 400-for-non-git path are both exercisable.
 */
export const MOCK_GIT_REPO_ROOT = '/home/dev/projects/delta';
const MOCK_GIT_DEFAULT_BRANCH = 'main';
const MOCK_GIT_REMOTE_BRANCHES = ['main', 'develop', 'release/1.0'];

/** Whether `path` is inside the mock git repository. */
function isInMockGitRepo(path: string): boolean {
  return path === MOCK_GIT_REPO_ROOT || path.startsWith(`${MOCK_GIT_REPO_ROOT}/`);
}

/**
 * The mock `GET /api/workdir/git` answer for `path`: a `repo_root` +
 * `default_branch` when the path is inside the mock repo, both `null`
 * otherwise. Mirrors the real endpoint, which never errors (it just reports
 * "not a git repo" as `repo_root: null`).
 */
export function gitRepoInfo(path: string): {
  repo_root: string | null;
  default_branch: string | null;
} {
  if (isInMockGitRepo(path)) {
    return {
      repo_root: MOCK_GIT_REPO_ROOT,
      default_branch: MOCK_GIT_DEFAULT_BRANCH,
    };
  }
  return { repo_root: null, default_branch: null };
}

/**
 * The mock `GET /api/workdir/git/branches` answer for `path`, or `null` when the
 * path is not a git repository (mapped to a 400 by the handler, matching the
 * real endpoint that rejects a non-git path).
 */
export function gitBranches(path: string): {
  default_branch: string | null;
  remote_branches: string[];
} | null {
  if (!isInMockGitRepo(path)) {
    return null;
  }
  return {
    default_branch: MOCK_GIT_DEFAULT_BRANCH,
    remote_branches: [...MOCK_GIT_REMOTE_BRANCHES],
  };
}


/** Mock pull requests for the new-session PR tab, shared by `reviewer`
 *  and `author` lens fixtures. The first PR is on a repo with a
 *  registered local clone (`x7c1/delta`, see `mockRepositories`); the
 *  second is on a repo with no local clone (`x7c1/other`) so the
 *  no-clone path — the inline clone panel — is exercisable alongside
 *  the happy path. */
export function mockReviewerPullRequests(): {
  number: number;
  title: string;
  repo_owner: string;
  repo_name: string;
  head_ref: string;
  head_repo_owner: string;
  head_repo_name: string;
  draft: boolean;
  url: string;
  updated_at: string;
  author_login: string;
  has_local_clone: boolean;
}[] {
  return [
    {
      number: 174,
      title: 'feat: add Repository tab to the new-session screen',
      repo_owner: 'x7c1',
      repo_name: 'delta',
      head_ref: 'feat/repo-tab',
      head_repo_owner: 'x7c1',
      head_repo_name: 'delta',
      draft: false,
      url: 'https://github.com/x7c1/delta/pull/174',
      updated_at: '2026-06-20T11:33:21Z',
      author_login: 'collaborator',
      has_local_clone: true,
    },
    {
      number: 9,
      title: 'fix: something obscure',
      repo_owner: 'x7c1',
      repo_name: 'other',
      head_ref: 'fix/obscure',
      head_repo_owner: 'x7c1',
      head_repo_name: 'other',
      draft: false,
      url: 'https://github.com/x7c1/other/pull/9',
      updated_at: '2026-06-19T08:00:00Z',
      author_login: 'collaborator',
      has_local_clone: false,
    },
  ];
}

export function mockAuthorPullRequests(): ReturnType<
  typeof mockReviewerPullRequests
> {
  return [
    {
      number: 200,
      title: 'wip: my own draft',
      repo_owner: 'x7c1',
      repo_name: 'delta',
      head_ref: 'feat/my-draft',
      head_repo_owner: 'x7c1',
      head_repo_name: 'delta',
      draft: true,
      url: 'https://github.com/x7c1/delta/pull/200',
      updated_at: '2026-06-24T01:00:00Z',
      author_login: 'x7c1',
      has_local_clone: true,
    },
  ];
}

/** Mock repositories for the new-session Repository tab. The default
 *  bundles two clones under one origin URL; the second is a single-clone
 *  entry whose `origin` was unset so it falls back to a path-keyed entry. */
export function mockRepositories(): {
  identity_key: string;
  display_name: string;
  recently_used_clone_path: string;
  clones: {
    path: string;
    last_opened_at: string | null;
    last_branch: string | null;
    last_launch_option_ids: number[];
    last_worktree_enabled: boolean;
    last_worktree_start_point: null;
  }[];
}[] {
  return [
    {
      identity_key: 'github.com/x7c1/delta',
      display_name: 'x7c1/delta',
      recently_used_clone_path: '/home/dev/projects/delta',
      clones: [
        {
          path: '/home/dev/projects/delta',
          last_opened_at: '2026-01-03T00:00:00Z',
          last_branch: 'main',
          last_launch_option_ids: [],
          last_worktree_enabled: false,
          last_worktree_start_point: null,
        },
        {
          path: '/home/dev/projects/delta-fork',
          last_opened_at: '2026-01-02T00:00:00Z',
          last_branch: 'feature/x',
          last_launch_option_ids: [],
          last_worktree_enabled: false,
          last_worktree_start_point: null,
        },
      ],
    },
    {
      identity_key: '/home/dev/projects/website',
      display_name: 'website',
      recently_used_clone_path: '/home/dev/projects/website',
      clones: [
        {
          path: '/home/dev/projects/website',
          last_opened_at: '2026-01-01T00:00:00Z',
          last_branch: 'main',
          last_launch_option_ids: [],
          last_worktree_enabled: false,
          last_worktree_start_point: null,
        },
      ],
    },
  ];
}

/**
 * A scripted comms log for the mock-mode Codex session: one launch plus one turn
 * of the frames Delta and a `codex app-server` actually exchange.
 *
 * MSW cannot mock WebSockets, so mock mode has no `/comms` stream to read (the
 * same reason `/ws` is replaced by {@link FakeEventSource}). This is the
 * equivalent stand-in, which is what makes the comms pane demonstrable with no
 * backend — in `make mock` and in the end-to-end suite alike.
 *
 * Deliberately covers every (direction, kind) pair the real stream carries, so a
 * rendering bug in any one of them shows up: Delta's requests and the responses
 * to them, the server's pushed notifications, its own approval request, and
 * Delta's answer to that request. Sequence numbers are contiguous from 0 and
 * timestamps increase, exactly as the server mints them.
 */
export function mockCommsFrames(): CommsFrame[] {
  const startedAt = Date.UTC(2026, 0, 1, 9, 30, 0);
  const frames: [CommsDirection, CommsFrameKind, string | null, unknown][] = [
    ['to_agent', 'request', 'thread/start', { id: 1, method: 'thread/start', params: { cwd: '/home/dev/projects/delta' } }],
    ['from_agent', 'response', 'thread/start', { id: 1, result: { thread: { id: 'thr_mock_0001' }, model: 'gpt-5.6-sol' } }],
    ['to_agent', 'request', 'turn/start', { id: 2, method: 'turn/start', params: { threadId: 'thr_mock_0001', input: [{ type: 'text', text: 'add a test' }] } }],
    ['from_agent', 'response', 'turn/start', { id: 2, result: { turn: { id: 'turn_mock_0001' } } }],
    ['from_agent', 'notification', 'turn/started', { method: 'turn/started', params: { threadId: 'thr_mock_0001', turnId: 'turn_mock_0001' } }],
    // A streaming burst — long enough for the pane to fold it into a group row.
    ['from_agent', 'notification', 'item/agentMessage/delta', { method: 'item/agentMessage/delta', params: { threadId: 'thr_mock_0001', itemId: 'm1', delta: 'Adding' } }],
    ['from_agent', 'notification', 'item/agentMessage/delta', { method: 'item/agentMessage/delta', params: { threadId: 'thr_mock_0001', itemId: 'm1', delta: ' the' } }],
    ['from_agent', 'notification', 'item/agentMessage/delta', { method: 'item/agentMessage/delta', params: { threadId: 'thr_mock_0001', itemId: 'm1', delta: ' test' } }],
    ['from_agent', 'notification', 'item/agentMessage/delta', { method: 'item/agentMessage/delta', params: { threadId: 'thr_mock_0001', itemId: 'm1', delta: ' now.' } }],
    ['from_agent', 'request', 'item/commandExecution/requestApproval', { id: 'srv-1', method: 'item/commandExecution/requestApproval', params: { threadId: 'thr_mock_0001', itemId: 'exec_1', command: 'cargo test' } }],
    ['to_agent', 'response', null, { id: 'srv-1', result: { decision: 'accept' } }],
    ['from_agent', 'notification', 'item/completed', { method: 'item/completed', params: { threadId: 'thr_mock_0001', item: { id: 'm1', type: 'agentMessage', text: 'Added the test.' } } }],
    ['from_agent', 'notification', 'turn/completed', { method: 'turn/completed', params: { threadId: 'thr_mock_0001', turnId: 'turn_mock_0001', status: 'completed' } }],
  ];
  return frames.map(([direction, kind, method, payload], index) => ({
    seq: index,
    at_ms: startedAt + index * 120,
    direction,
    kind,
    method,
    payload_json: JSON.stringify(payload),
  }));
}

/**
 * Per-provider launch availability and capability profile for
 * `GET /api/providers`. Both providers are available by default so the
 * new-session provider selector is fully usable with no backend and existing
 * tests / e2e are unaffected. Capabilities mirror the real backend: Claude
 * offers an attachable terminal (`has_terminal: true`) and takes its launch
 * options as CLI flags, Codex is headless (`has_terminal: false`, and therefore
 * `has_comms_log: true` — its right-pane window is the frame log instead) and
 * takes them as session-start request fields. The workspace picks the right pane
 * from those two flags, and Settings words its launch-option form from the style.
 * Codex also accepts a session-scoped permission decision
 * (`has_allow_for_session: true`), which is what puts the "Allow for session"
 * button on its approval notices and keeps it off Claude's.
 * A test that needs an unavailable provider overrides this handler (see
 * `createHandlers`).
 */
export function mockProviders(): ProviderAvailability[] {
  return [
    {
      provider: 'claude',
      available: true,
      detail: null,
      capabilities: {
        has_terminal: true,
        has_comms_log: false,
        has_allow_for_session: false,
        launch_option_style: 'cli_flag',
      },
    },
    {
      provider: 'codex',
      available: true,
      detail: null,
      capabilities: {
        has_terminal: false,
        has_comms_log: true,
        has_allow_for_session: true,
        launch_option_style: 'request_field',
      },
    },
  ];
}
