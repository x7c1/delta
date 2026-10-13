/**
 * Shared fixtures and helpers for the `WorkspaceScreen.*.test.tsx` family: the
 * MSW server, the render and event helpers, the spawn and sender helpers, the
 * single-session list overrides, and the per-case store reset.
 *
 * Not a test file itself: the `.testkit.` segment keeps it outside vitest's
 * `*.test.*` include glob. The module mocks stay in each test file, since
 * vitest hoists `vi.mock` per file.
 */
import { vi } from 'vitest';
import type { ReactNode } from 'react';
import {
  act,
  render,
  screen,
  within,
  type RenderResult,
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import {
  SESSION_ID,
  SESSION_ID_2,
  SESSION_ID_4,
  SESSION_2_MAIN_THREAD_ID,
  createHandlers,
  mockThreads,
  mockThreads2,
  mockThreads4,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { SessionEvent, Thread } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { applySessionEvent } from '../../data/applySessionEvent';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';
import { useNewSessionSend } from '../composer/useNewSessionSend';
import { WorkspaceScreen } from './WorkspaceScreen';

/**
 * The threads a session-list item carries for `id`: the mock store's own tree
 * for a seeded session (what its threads route answers), else the trunk alone.
 */
export function listedThreads(id: string, mainThreadId: number): Thread[] {
  const seeded: Record<string, Thread[]> = {
    [SESSION_ID]: mockThreads,
    [SESSION_ID_2]: mockThreads2,
    [SESSION_ID_4]: mockThreads4,
  };
  return (
    seeded[id] ?? [
      {
        id: mainThreadId,
        session_id: id,
        title: 'main',
        parent_thread_id: null,
        root_message_uuid: null,
        created_at: '2026-01-01T00:00:00Z',
        last_activity_at: null,
      },
    ]
  );
}

// jsdom does not implement matchMedia, which `useMediaQuery` relies on.
export function stubMatchMedia(): void {
  vi.stubGlobal(
    'matchMedia',
    (query: string) =>
      ({
        matches: false,
        media: query,
        onchange: null,
        addEventListener: () => {},
        removeEventListener: () => {},
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList,
  );
}

export const server = setupServer(...createHandlers());

/** Render the workspace, optionally with a test harness beside it. */
export function renderScreen(
  harness?: ReactNode,
): RenderResult & { queryClient: QueryClient } {
  const queryClient = new QueryClient({
    // `retryDelay: 0` because the session list sets its own `retry`, which
    // overrides the `retry: false` default; without it a failing sessions
    // request would sit through TanStack's back-off before settling.
    defaultOptions: { queries: { retry: false, retryDelay: 0 } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return {
    queryClient,
    ...render(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={client}>
          <WorkspaceScreen />
          {harness}
        </ApiProvider>
      </QueryClientProvider>,
    ),
  };
}

/**
 * Deliver a live `SessionEvent` to the rendered workspace exactly as
 * `useSessionEvents` does — reading the CURRENT focus out of the nav store at
 * event time (with `NEW_SESSION_FOCUS` mapped to null) rather than capturing it
 * up front. `useSessionEvents` itself is stubbed out in this suite, so the
 * routing has to be reproduced here; reading focus live is the part that
 * matters, since the unread rules are decided from it.
 */
export function deliverEvent(queryClient: QueryClient, event: SessionEvent) {
  const { activeThreadId, focusedSessionId } = useNavStore.getState();
  act(() => {
    applySessionEvent(
      event,
      queryClient,
      activeThreadId,
      focusedSessionId === null || focusedSessionId === NEW_SESSION_FOCUS
        ? null
        : focusedSessionId,
    );
  });
}

/**
 * The navigator row for the sub-thread labelled `title`. Scoped to the session
 * card because the transcript pane names the active thread too — an unscoped
 * lookup matches both once that thread is the one on screen. The label span is
 * nested inside the row button, so walk up to the button: that is the element
 * carrying the thread's unread badge.
 */
export function threadRow(title: string): HTMLElement {
  const row = within(screen.getByTestId('session-card'))
    .getByText(title)
    .closest('button');
  if (row === null) {
    throw new Error(`no navigator row for thread "${title}"`);
  }
  return row;
}

/**
 * A session id the mock backend never lists — the state a just-accepted spawn
 * is in for the moment before its refetch lands. Using a listed id for that
 * case would hide exactly the bug these tests guard: focus is handed over on
 * the tracked spawn alone, never on the row being present.
 */
export const UNLISTED_SPAWN_ID = 'sess-just-spawned';

/**
 * A second such id: the launch started from the new-session screen while the
 * first is still coming up — the thing the one-shot hand-over exists to allow.
 */
export const SECOND_SPAWN_ID = 'sess-just-spawned-2';

/** A tracked new-session spawn, as `useSubmitSend` records it from the POST. */
export function trackedSpawn(sessionId: string) {
  return {
    sessionId,
    threadId: SESSION_2_MAIN_THREAD_ID,
    text: 'first message',
    firstSendId: 1,
    workdir: null,
    launchOptionIds: [],
    provider: 'claude' as const,
    worktree: null,
    pullRequestNumber: null,
    status: 'spawning' as const,
  };
}

/** The harness button's label (see {@link NewSessionSender}). */
export const SENDER_LABEL = 'start a session from the test';

/**
 * A button that starts a new session through the real submit path. It is
 * rendered beside the workspace rather than driven through the composer
 * because the cases below have to send from screens that carry no new-session
 * composer at all — the whole point being where focus is when the POST lands.
 */
function NewSessionSender() {
  const send = useNewSessionSend();
  return (
    <button
      type="button"
      onClick={() =>
        send({
          text: 'first message',
          workdir: '/work',
          launchOptionIds: [],
          provider: 'claude',
          worktree: null,
          pullRequestNumber: null,
        })
      }
    >
      {SENDER_LABEL}
    </button>
  );
}

/** Render the workspace with the {@link NewSessionSender} harness beside it. */
export function renderScreenWithSender(): RenderResult & {
  queryClient: QueryClient;
} {
  return renderScreen(<NewSessionSender />);
}

/**
 * Accept the next `POST /api/sends` into `sessionId`, as the server does for a
 * new session: real ids on the response, and the row not yet in the loaded
 * list. Stubbed rather than served by the mock store so the session the send
 * creates cannot leak into later specs' session lists.
 */
export function acceptNextSendAs(sessionId: string) {
  server.use(
    http.post('*/api/sends', () =>
      HttpResponse.json(
        {
          send: {
            id: 1,
            session_id: sessionId,
            thread_id: SESSION_2_MAIN_THREAD_ID,
            semantic_parent_uuid: null,
            text: 'first message',
            locator_quote: null,
            status: 'dispatched',
            matched_uuid: null,
            created_at: '2026-01-01T00:00:00Z',
            held_at: null,
          },
        },
        { status: 201 },
      ),
    ),
  );
}

/** Reset the stores every multi-session case starts from. */
export function resetWorkspaceState(): void {
  useNavStore.setState({
    focusedSessionId: null,
    activeThreadId: null,
    preNewSessionFocus: null,
    settingsOpen: false,
    terminalOpenBySession: {},
    terminalOpenWithoutSession: false,
    commsOpen: false,
  });
  useComposerStore.setState({
    drafts: {},
    branchOrigin: null,
    newSessionWorkdir: null,
    workdirDialogOpen: false,
  });
  // `notices` is reset alongside the rest: a case that seeds a permission
  // notice would otherwise leak the card into every later case's screen.
  useLiveStore.setState({
    spawns: [],
    unread: {},
    notices: {},
    resolvedRequests: {},
  });
}

// A single-session `/api/sessions` override for a given provider, so a test
// does not depend on the shared mock store's mutated state or on pagination.
// The focused session's threads still resolve through the default handler
// (the store keeps every seed session's threads). Originally added for the
// terminal-gating tests below; also used above by the unread-badge tests,
// which need exactly one session card rendered for `threadRow` to scope to —
// without it they fail on "found multiple elements", not on a badge.
/** The options of {@link singleSessionList}. */
interface SingleSessionState {
  open?: boolean;
  status?: 'active' | 'spawning' | 'failed';
  paneStarting?: boolean;
  hooksUnreachable?: boolean;
}

/** A `GET /api/sessions` body listing exactly one session. */
export function singleSessionList(
  id: string,
  provider: 'claude' | 'codex',
  mainThreadId: number,
  // Named rather than three trailing positionals: which state the caller is
  // after (open, `spawning` with a pane, `spawning` without one, failed) is
  // the point of every test that passes them, and `(…, false, 'spawning',
  // true)` does not say.
  {
    open = true,
    status = 'active',
    paneStarting = false,
    hooksUnreachable = false,
  }: SingleSessionState = {},
) {
  return {
    sessions: [
      {
        session: {
          id,
          cwd: '/work',
          transcript_path: '/tmp/s.jsonl',
          title: `${provider} session`,
          status,
          created_at: '2026-01-01T00:00:00Z',
          branch_at_launch: null,
          repo_root: null,
          repository_display_name: null,
          provider,
          provider_session_id: null,
          provider_thread_id: null,
          pull_request_number: null,
        },
        open,
        pane_starting: paneStarting,
        hooks_unreachable: hooksUnreachable,
        main_thread_id: mainThreadId,
        threads: listedThreads(id, mainThreadId),
        last_activity_at: '2026-01-01T00:00:02Z',
      },
    ],
    next_cursor: null,
  };
}

export function useSingleSessionOfProvider(
  id: string,
  provider: 'claude' | 'codex',
  mainThreadId: number,
  state: SingleSessionState = {},
) {
  server.use(
    http.get('*/api/sessions', () =>
      HttpResponse.json(singleSessionList(id, provider, mainThreadId, state)),
    ),
  );
}
