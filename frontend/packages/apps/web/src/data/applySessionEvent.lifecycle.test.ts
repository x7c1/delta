/**
 * Session lifecycle events: spawn failures and their snackbars, session
 * removal, permission routing, and clone outcomes.
 *
 * Shared fixtures live in applySessionEvent.testkit.ts.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { QueryClient } from '@tanstack/react-query';
import { queryKeys } from '@delta/api-client';
import { applySessionEvent } from './applySessionEvent';
import { noticeOf, useLiveStore } from '../store/liveStore';
import { NEW_SESSION_FOCUS, useNavStore } from '../store/navStore';
import { useNotificationStore } from '../store/notificationStore';
import {
  FOCUSED,
  resetApplySessionEventState,
  seedSessionList,
  serverSend,
} from './applySessionEvent.testkit';

describe('applySessionEvent', () => {
  beforeEach(resetApplySessionEventState);

  it('fails the tracked spawn and refetches its sends on spawn_failed', () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');
    // The spawn was accepted with real ids; its first send is in the cache.
    useLiveStore.getState().trackSpawn({
      focusHandedOver: false,
      sessionId: 'sess-spawned',
      threadId: 42,
      text: 'new session',
      firstSendId: 1,
      workdir: '/projects/app',
      launchOptionIds: [],
      provider: 'claude',
      worktree: null,
      pullRequestNumber: null,
    });
    queryClient.setQueryData(queryKeys.sessionSends('sess-spawned'), {
      sends: [serverSend({ session_id: 'sess-spawned', thread_id: 42 })],
    });

    applySessionEvent(
      {
        kind: 'spawn_failed',
        cancelled: false,
        session_id: 'sess-spawned',
        pane_token: 'pane-1',
      },
      queryClient,
      null,
      FOCUSED,
    );

    expect(useLiveStore.getState().spawns[0].status).toBe('failed');
    // The row is kept, so its open sends are still real: refetch them rather
    // than dropping them — they are the messages the failed session's own
    // screen shows.
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: queryKeys.sessionSends('sess-spawned'),
    });
    // The session's status and its place in the list both moved, so the list
    // must refetch.
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['sessions'] });
    // The failure named a session other than the focused one, so focus stays.
    expect(useNavStore.getState().focusedSessionId).toBe(FOCUSED);
  });

  it('refetches the session list on spawn_pane_ready', () => {
    // The launch's pane came up. The row reads `spawning` on both sides of that,
    // but its `pane_starting` flag flips with it — and that flag is the only
    // thing the browser has to decide whether the embedded terminal may attach.
    // So this event carries no state of its own: it is a nudge to refetch the
    // row that does.
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      {
        kind: 'spawn_pane_ready',
        session_id: 'sess-spawned',
        pane_token: 'delta-1',
      },
      queryClient,
      null,
      FOCUSED,
    );

    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['sessions'] });
  });

  it('leaves focus on the failed session when it is the focused one', () => {
    // The workspace focuses a spawn as soon as its send is accepted, so a
    // failure usually lands on the session the user is looking at. That screen
    // is no longer about to describe a session that stopped existing: the row
    // is kept, and the screen now shows the failure, what was never sent, and
    // Retry / Remove. So nothing teleports the user away from it.
    const queryClient = new QueryClient();
    useNavStore.setState({ focusedSessionId: 'sess-spawned' });
    useLiveStore.getState().trackSpawn({
      focusHandedOver: false,
      sessionId: 'sess-spawned',
      threadId: 42,
      text: 'new session',
      firstSendId: 1,
      workdir: '/projects/app',
      launchOptionIds: [],
      provider: 'claude',
      worktree: null,
      pullRequestNumber: null,
    });

    applySessionEvent(
      {
        kind: 'spawn_failed',
        cancelled: false,
        session_id: 'sess-spawned',
        pane_token: 'pane-1',
      },
      queryClient,
      null,
      'sess-spawned',
    );

    expect(useNavStore.getState().focusedSessionId).toBe('sess-spawned');
    expect(useLiveStore.getState().spawns[0].status).toBe('failed');
  });

  /** Track a launch this window started, still starting, under `sessionId`. */
  function trackStartingSpawn(sessionId: string, text = 'new session') {
    useLiveStore.getState().trackSpawn({
      focusHandedOver: false,
      sessionId,
      threadId: 42,
      text,
      firstSendId: 1,
      workdir: '/projects/app',
      launchOptionIds: [],
      provider: 'claude',
      worktree: null,
      pullRequestNumber: null,
    });
  }

  it('names a tracked launch that fails while another session is focused', () => {
    // The user started a session, moved on to another one while it was
    // starting, and then it broke. The main pane shows the other session and
    // the failed card may be scrolled out of view, so the snackbar is the only
    // thing that tells them — naming the session by the prompt they wrote.
    const queryClient = new QueryClient();
    trackStartingSpawn('sess-spawned', 'Fix the flaky login test');

    applySessionEvent(
      {
        kind: 'spawn_failed',
        cancelled: false,
        session_id: 'sess-spawned',
        pane_token: 'pane-1',
        reason: 'git error: worktree add failed\n\n<captured pane>',
      },
      queryClient,
      null,
      FOCUSED,
    );

    expect(useNotificationStore.getState().notifications).toEqual([
      expect.objectContaining({
        tone: 'error',
        title: 'Session “Fix the flaky login test” failed to start',
        detail: 'git error: worktree add failed',
        // The card shows no prompt, so the name alone would not find it.
        action: { kind: 'open-session', sessionId: 'sess-spawned' },
      }),
    ]);
  });

  it('shortens a long first prompt to one line when naming the failed launch', () => {
    const queryClient = new QueryClient();
    trackStartingSpawn(
      'sess-spawned',
      'Refactor the session navigator so that\nlaunches stay pinned until they bind',
    );
    // Not on any session at all: the new-session screen routes as no focus.
    applySessionEvent(
      {
        kind: 'spawn_failed',
        cancelled: false,
        session_id: 'sess-spawned',
        pane_token: 'pane-1',
      },
      queryClient,
      null,
      null,
    );

    expect(useNotificationStore.getState().notifications).toEqual([
      expect.objectContaining({
        title:
          'Session “Refactor the session navigator so that…” failed to start',
        // No reason to show: point at the session's own screen.
        detail: 'Open it to see why.',
        action: { kind: 'open-session', sessionId: 'sess-spawned' },
      }),
    ]);
  });

  it('raises no snackbar for a tracked launch that fails while it is focused', () => {
    // The failed session's own screen is on view and says what happened.
    const queryClient = new QueryClient();
    useNavStore.setState({ focusedSessionId: 'sess-spawned' });
    trackStartingSpawn('sess-spawned');

    applySessionEvent(
      {
        kind: 'spawn_failed',
        cancelled: false,
        session_id: 'sess-spawned',
        pane_token: 'pane-1',
        reason: 'git error: worktree add failed',
      },
      queryClient,
      null,
      'sess-spawned',
    );

    expect(useNotificationStore.getState().notifications).toEqual([]);
  });

  it('raises no snackbar for a tracked launch the user cancelled', () => {
    // Close on a starting session ends its launch with `cancelled: true`. The
    // user asked for that, wherever they are looking now.
    const queryClient = new QueryClient();
    trackStartingSpawn('sess-spawned');

    applySessionEvent(
      {
        kind: 'spawn_failed',
        cancelled: true,
        session_id: 'sess-spawned',
        pane_token: 'pane-1',
      },
      queryClient,
      null,
      FOCUSED,
    );

    expect(useNotificationStore.getState().notifications).toEqual([]);
  });

  it('raises the tracked-launch snackbar once on a repeated spawn_failed', () => {
    const queryClient = new QueryClient();
    trackStartingSpawn('sess-spawned');
    const event = {
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawned',
      pane_token: 'pane-1',
    } as const;

    applySessionEvent(event, queryClient, null, FOCUSED);
    applySessionEvent(event, queryClient, null, FOCUSED);

    expect(useNotificationStore.getState().notifications).toHaveLength(1);
  });

  it.each([
    { cancelled: false, raised: true },
    { cancelled: true, raised: false },
  ])(
    'names a launch whose failure outran its POST (cancelled: $cancelled)',
    ({ cancelled, raised }) => {
      // A launch preparation that breaks at once can report before the POST
      // response does. The failure is buffered, `trackSpawn` registers the
      // launch already failed, and the workspace never focuses it — so the
      // user is left on the new-session screen with only this to tell them.
      const queryClient = new QueryClient();
      useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
      useLiveStore.getState().beginSending({
        id: 'local-new-session',
        target: {
          kind: 'new-session',
          workdir: '/projects/app',
          launchOptionIds: [],
          provider: 'claude',
          worktree: null,
          pullRequestNumber: null,
        },
        text: 'Fix the flaky login test',
        status: 'sending',
        createdAt: Date.now(),
      });
      applySessionEvent(
        {
          kind: 'spawn_failed',
          cancelled,
          session_id: 'sess-spawned',
          pane_token: 'pane-1',
          reason: 'git error: worktree add failed\n\n<captured pane>',
        },
        queryClient,
        null,
        null,
      );
      expect(useNotificationStore.getState().notifications).toEqual([]);

      trackStartingSpawn('sess-spawned', 'Fix the flaky login test');

      expect(useLiveStore.getState().spawns[0].status).toBe('failed');
      expect(useNotificationStore.getState().notifications).toEqual(
        raised
          ? [
              expect.objectContaining({
                tone: 'error',
                title: 'Session “Fix the flaky login test” failed to start',
                // The buffered notice kept the reason for this snackbar.
                detail: 'git error: worktree add failed',
                action: { kind: 'open-session', sessionId: 'sess-spawned' },
              }),
            ]
          : [],
      );
    },
  );

  it('drops the removed session and hands focus to the next one on session_removed', () => {
    // The row is gone server-side, so the cached open sends go with it (a
    // refetch would only 404) and the session list must refetch to lose the
    // card. Focus goes to the first OTHER session in the cached list — read
    // before the invalidation, so the choice cannot race the refetch.
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');
    seedSessionList(queryClient, [FOCUSED, 'sess-2', 'sess-3']);
    queryClient.setQueryData(queryKeys.sessionSends(FOCUSED), {
      sends: [serverSend()],
    });

    applySessionEvent(
      { kind: 'session_removed', session_id: FOCUSED },
      queryClient,
      null,
      FOCUSED,
    );

    expect(
      queryClient.getQueryData(queryKeys.sessionSends(FOCUSED)),
    ).toBeUndefined();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['sessions'] });
    expect(useNavStore.getState().focusedSessionId).toBe('sess-2');
  });

  it('falls back to the new-session screen when the removed session was the last one', () => {
    // Nothing else to focus, so the user lands where a session is started
    // rather than on a screen describing a session that no longer exists.
    const queryClient = new QueryClient();
    seedSessionList(queryClient, [FOCUSED]);

    applySessionEvent(
      { kind: 'session_removed', session_id: FOCUSED },
      queryClient,
      null,
      FOCUSED,
    );

    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
  });

  it('leaves focus alone when a different session is removed', () => {
    // Another tab (or another card in this one) removed a session the user is
    // not looking at: the only thing that happens on their screen is the card
    // disappearing. Moving focus would yank them out of what they are reading.
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');
    seedSessionList(queryClient, [FOCUSED, 'sess-2']);

    applySessionEvent(
      { kind: 'session_removed', session_id: 'sess-2' },
      queryClient,
      null,
      FOCUSED,
    );

    expect(invalidate).toHaveBeenCalledWith({ queryKey: ['sessions'] });
    expect(useNavStore.getState().focusedSessionId).toBe(FOCUSED);
  });

  it('withdraws a snackbar that offers to open a removed session', () => {
    // Retry removes the failed row, as does Remove (here or in another tab).
    // An Open left on screen would focus an id that no longer exists.
    const queryClient = new QueryClient();
    seedSessionList(queryClient, [FOCUSED, 'sess-2']);
    const notifications = useNotificationStore.getState();
    notifications.showError('Session “a” failed to start', undefined, {
      kind: 'open-session',
      sessionId: 'sess-2',
    });
    notifications.showError('Session “b” failed to start', undefined, {
      kind: 'open-session',
      sessionId: 'sess-3',
    });
    notifications.showError('Could not open in VS Code');

    applySessionEvent(
      { kind: 'session_removed', session_id: 'sess-2' },
      queryClient,
      null,
      FOCUSED,
    );

    expect(
      useNotificationStore.getState().notifications.map((n) => n.title),
    ).toEqual(['Session “b” failed to start', 'Could not open in VS Code']);
  });

  it('routes a permission request to the store as a notice', () => {
    const queryClient = new QueryClient();
    applySessionEvent(
      {
        kind: 'permission_requested',
        session_id: FOCUSED,
        request_id: 2,
        tool_name: 'Edit',
        tool_input: '{}',
      },
      queryClient,
      1,
      FOCUSED,
    );

    expect(
      noticeOf(useLiveStore.getState().notices, FOCUSED, 'permission'),
    ).toEqual({
      kind: 'permission',
      requestId: 2,
      toolName: 'Edit',
      toolInput: '{}',
      dismissed: false,
      queued: [],
      pendingCount: 1,
    });
  });

  it('refetches the open sends on permission_resolved', () => {
    // A sends envelope in flight when the resolution lands still reports the
    // request as pending; the refetch supersedes it — see the
    // `permission_resolved` case in `applySessionEvent`.
    const queryClient = new QueryClient();
    applySessionEvent(
      {
        kind: 'question_asked',
        session_id: FOCUSED,
        request_id: 4,
        thread_id: 1,
        tool_input: '{"questions":[]}',
      },
      queryClient,
      1,
      FOCUSED,
    );
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      { kind: 'permission_resolved', session_id: FOCUSED, request_id: 4 },
      queryClient,
      1,
      FOCUSED,
    );

    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ['session-sends', FOCUSED],
    });
    expect(
      noticeOf(useLiveStore.getState().notices, FOCUSED, 'question'),
    ).toBeNull();
  });

  it('refetches the repository and PR lists on a clone outcome, with no session in the event', () => {
    // The clone events name no session — routing them by `session_id` would
    // throw, and nothing about them is focus-dependent. They refetch because
    // whether a clone exists is a fact about the filesystem that this browser
    // (or another one) may have just changed.
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      {
        kind: 'repository_clone_completed',
        repo_owner: 'x7c1',
        repo_name: 'delta',
        clone_root: '/home/dev/projects',
        destination_path: '/home/dev/projects/delta',
      },
      queryClient,
      null,
      null,
    );

    expect(invalidate).toHaveBeenCalledWith({
      queryKey: queryKeys.repositories,
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: queryKeys.pullRequests('reviewer'),
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: queryKeys.pullRequests('author'),
    });
  });

  it('refetches the same lists on a failed clone', () => {
    // A failure changes nothing on disk, but the lists are refetched anyway:
    // this browser cannot tell whether the failure was the only thing that
    // happened, and a stale "no clone" row is worse than one extra fetch.
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, 'invalidateQueries');

    applySessionEvent(
      {
        kind: 'repository_clone_failed',
        repo_owner: 'x7c1',
        repo_name: 'delta',
        clone_root: '/home/dev/projects',
        destination_path: '/home/dev/projects/delta',
        message: 'could not resolve host github.com',
      },
      queryClient,
      null,
      null,
    );

    expect(invalidate).toHaveBeenCalledWith({
      queryKey: queryKeys.repositories,
    });
  });
});
