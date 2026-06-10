import type {
  Message,
  PendingSend,
  Session,
  Thread,
} from '@delta/model';

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
 */

export const SESSION_ID = 'sess-mock-1';
export const SESSION_ID_2 = 'sess-mock-2';
export const SESSION_ID_3 = 'sess-mock-3';
export const MAIN_THREAD_ID = 1;
export const BRANCH_THREAD_ID = 2;
export const SESSION_2_MAIN_THREAD_ID = 3;
export const SESSION_2_BRANCH_THREAD_ID = 4;
export const SESSION_3_MAIN_THREAD_ID = 5;

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

/** Total seeded sessions: the three detailed ones plus the filler. */
export const TOTAL_SEEDED_SESSIONS = 3 + FILLER_SESSION_COUNT;

export const mockSession: Session = {
  id: SESSION_ID,
  cwd: '/work/delta',
  transcript_path: '/tmp/transcript.jsonl',
  title: null,
  status: 'active',
  created_at: '2026-01-01T00:00:00Z',
};

export const mockSession2: Session = {
  id: SESSION_ID_2,
  cwd: '/work/scratch',
  transcript_path: '/tmp/transcript-2.jsonl',
  title: 'scratch notes',
  status: 'ended',
  created_at: '2026-01-02T00:00:00Z',
};

export const mockSession3: Session = {
  id: SESSION_ID_3,
  cwd: '/work/old-experiment',
  transcript_path: '/tmp/transcript-3-gone.jsonl',
  title: 'resume-unavailable demo',
  status: 'ended',
  created_at: '2025-12-31T00:00:00Z',
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
    },
  ],
};

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
  }[];
  messagesByThread: Record<number, Message[]>;
  sends: PendingSend[];
  nextThreadId: number;
  nextSendId: number;
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
      ...structuredClone(filler),
    ],
    messagesByThread: structuredClone(mockMessagesByThread),
    sends: [],
    nextThreadId: FIRST_FILLER_THREAD_ID + FILLER_SESSION_COUNT,
    nextSendId: 1,
  };
}

/** The mock filesystem's `$HOME`, used when a workdir list omits `path`. */
export const MOCK_WORKDIR_HOME = '/home/dev';

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
