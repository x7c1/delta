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
 * - `sess-mock-2` — **closed**. A separate session with its own `main` thread
 *   and a short transcript, used to verify a closed session renders read-only.
 */

export const SESSION_ID = 'sess-mock-1';
export const SESSION_ID_2 = 'sess-mock-2';
export const MAIN_THREAD_ID = 1;
export const BRANCH_THREAD_ID = 2;
export const SESSION_2_MAIN_THREAD_ID = 3;

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
 */
const FILLER_SESSION_COUNT = 4;
const FIRST_FILLER_THREAD_ID = 100;

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
};

/** In-memory store shape shared by the MSW handlers within one render. */
export interface MockStore {
  /** Sessions keyed by id, each with its open flag, threads, and main thread. */
  sessions: {
    session: Session;
    open: boolean;
    mainThreadId: number;
    threads: Thread[];
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
    // Descending dates (2025-12-...) keep them older than the detailed sessions
    // and give each a deterministic, distinct recency for stable pagination.
    const createdAt = `2025-12-${String(31 - index).padStart(2, '0')}T00:00:00Z`;
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
      ...structuredClone(filler),
    ],
    messagesByThread: structuredClone(mockMessagesByThread),
    sends: [],
    nextThreadId: FIRST_FILLER_THREAD_ID + FILLER_SESSION_COUNT,
    nextSendId: 1,
  };
}
