import type {
  LaunchOption,
  Message,
  PendingPermission,
  PendingQuestion,
  PromptTemplate,
  RunningSubagent,
  Send,
  Session,
  StorageFile,
  StorageWorktree,
  Thread,
} from '@delta/wire-gen';
import {
  FILLER_SESSION_COUNT,
  FIRST_FILLER_THREAD_ID,
  MAIN_THREAD_ID,
  mockMessagesByThread,
  mockSession,
  mockSession2,
  mockSession3,
  mockSession4,
  mockThreads,
  mockThreads2,
  mockThreads3,
  mockThreads4,
  SESSION_2_MAIN_THREAD_ID,
  SESSION_3_MAIN_THREAD_ID,
  SESSION_4_MAIN_THREAD_ID,
} from './sessionFixtures';
import { mockStorage, mockStorageWorktrees } from './storageFixtures';

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
     * activates it — or a `spawn_failed` marks it `failed`. A plain send aimed at it
     * while it is starting is accepted as a `queued` row, and a branch send is
     * refused with `409 session_spawning`, as on the server.
     */
    spawning?: boolean;
    /**
     * True while a still-`spawning` row's pane is up but nothing has bound it
     * — the window in which the embedded terminal may attach to a launch that
     * has stopped on an interactive prompt. Mirrors the real server's
     * `pane_starting`, which is a fact about the row (not a one-shot event), so
     * a reloaded browser sees it too. Absent means no attachable pane.
     */
    paneStarting?: boolean;
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
  /**
   * The directories under the worktree base `GET /api/storage/worktrees`
   * lists; a `DELETE /api/storage/worktrees` takes one out.
   */
  storageWorktrees: StorageWorktree[];
  /**
   * The migration snapshots `GET /api/storage` lists; a
   * `DELETE /api/storage/snapshots` takes one out.
   */
  snapshots: StorageFile[];
  /**
   * Whether `POST /api/storage/erase` has run. The real server stops once it
   * answers; the mock keeps serving but refuses a second erase as the server
   * does while one is in progress.
   */
  erased: boolean;
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
        pull_request_number: null,
        failure_reason: null,
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
          last_activity_at: null,
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
        dangerous: false,
        choice_group: '--model',
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
        dangerous: false,
        choice_group: null,
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
        dangerous: false,
        choice_group: 'model',
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
        dangerous: false,
        choice_group: '--permission-mode',
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
        dangerous: false,
        choice_group: null,
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
    storageWorktrees: structuredClone(mockStorageWorktrees),
    snapshots: structuredClone(mockStorage.snapshots),
    erased: false,
  };
}
