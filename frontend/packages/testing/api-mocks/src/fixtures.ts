import type {
  Message,
  PendingSend,
  Session,
  Thread,
} from '@delta/model';

/**
 * A small but representative seed dataset so the UI is fully developable with
 * no backend: a trunk `main` thread with a multi-turn conversation (including a
 * tool call and a thinking block to exercise the collapsible blocks) and one
 * child branch thread sprouting from an assistant message.
 */

export const SESSION_ID = 'sess-mock-1';
export const MAIN_THREAD_ID = 1;
export const BRANCH_THREAD_ID = 2;

export const mockSession: Session = {
  id: SESSION_ID,
  cwd: '/work/delta',
  transcript_path: '/tmp/transcript.jsonl',
  title: null,
  status: 'active',
  created_at: '2026-01-01T00:00:00Z',
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
};

/** Build a fresh deep copy of the seed data so handlers can mutate freely. */
export function seedData(): {
  session: Session;
  threads: Thread[];
  messagesByThread: Record<number, Message[]>;
  sends: PendingSend[];
  nextThreadId: number;
  nextSendId: number;
} {
  return {
    session: structuredClone(mockSession),
    threads: structuredClone(mockThreads),
    messagesByThread: structuredClone(mockMessagesByThread),
    sends: [],
    nextThreadId: 3,
    nextSendId: 1,
  };
}
