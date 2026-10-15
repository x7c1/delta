import type { Message, Session, Thread } from '@delta/wire-gen';

/**
 * A small but representative multi-session seed so the UI is fully developable
 * with no backend. Two sessions in distinct lifecycle states exercise the
 * session list, the open/closed indicator, focusing a closed (view-only)
 * session, and the per-session thread tree:
 *
 * - `sess-mock-1` — **open**. A trunk `main` thread with a multi-turn
 *   conversation (a tool call and a thinking block, to exercise both card
 *   shapes) plus one child branch thread sprouting from an assistant message.
 * - `sess-mock-2` — **closed**. A separate session with its own `main` thread,
 *   a short transcript, and one child branch thread. It is on page 1 but is not
 *   the auto-focused session (the open `sess-mock-1` is), so it exercises a
 *   *non-focused* session showing its sub-thread tree expanded by default.
 * - `sess-mock-3` — **closed and resume-unavailable**. A readable session whose
 *   transcript is gone, so the backend would refuse to resume it. It is flagged
 *   `resumable: false`, which makes the mock `open`/`sends` handlers answer with
 *   `409 resume_unavailable` — exactly as the real server does — so the
 *   "this session cannot be resumed" UI is developable with no backend. It sorts
 *   just after the two detailed sessions (the last row of page 1, which holds
 *   the one live session plus `SESSIONS_PAGE_SIZE` closed ones), leaving the
 *   auto-focus unchanged.
 * - `sess-mock-4` — **closed, Codex provider**. The other three run on Claude;
 *   this one carries `provider: 'codex'` so the navigator's provider badge is
 *   exercisable for both providers with no backend. It has an empty main thread
 *   (no messages) and sorts just below `sess-mock-3` (top of page 2), again
 *   leaving the auto-focus unchanged.
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
export const FIRST_FILLER_THREAD_ID = 100;

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
  // Started from the PR tab: the navigator card links this number to
  // `https://github.com/dev/delta/pull/138`.
  pull_request_number: 138,
  failure_reason: null,
  closed_reason: null,
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
  // Started from a directory pick: no PR slot on the card.
  pull_request_number: null,
  failure_reason: null,
  closed_reason: null,
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
  pull_request_number: null,
  failure_reason: null,
  closed_reason: null,
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
  pull_request_number: null,
  failure_reason: null,
  closed_reason: null,
};

export const mockThreads: Thread[] = [
  {
    id: MAIN_THREAD_ID,
    session_id: SESSION_ID,
    title: 'main',
    parent_thread_id: null,
    root_message_uuid: null,
    last_activity_at: '2026-01-01T00:03:00Z',
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: BRANCH_THREAD_ID,
    session_id: SESSION_ID,
    title: 'delta etymology',
    parent_thread_id: MAIN_THREAD_ID,
    root_message_uuid: 'uuid-a1',
    // The newest activity in this session, so the navigator marks this row.
    last_activity_at: '2026-01-01T00:07:00Z',
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
    last_activity_at: '2026-01-02T00:02:00Z',
    created_at: '2026-01-02T00:00:00Z',
  },
  {
    id: SESSION_2_BRANCH_THREAD_ID,
    session_id: SESSION_ID_2,
    title: 'scratch ideas',
    parent_thread_id: SESSION_2_MAIN_THREAD_ID,
    root_message_uuid: 'uuid-s2-a1',
    last_activity_at: '2026-01-02T00:06:00Z',
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
    last_activity_at: null,
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
    last_activity_at: null,
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
