/**
 * Spawn tracking: new-session launches from acceptance to registration or
 * failure.
 *
 * Shared helpers live in liveStore.testkit.ts.
 */
import { beforeEach, describe, expect, it } from 'vitest';
import { noticeOf, useLiveStore } from './liveStore';
import { useNotificationStore } from './notificationStore';
import { localSend, notices, reset } from './liveStore.testkit';

describe('liveStore spawn tracking', () => {
  beforeEach(reset);

  function trackOne(sessionId = 'sess-spawn-1') {
    // The submit hook drops the in-flight chip before it registers the spawn.
    useLiveStore.getState().removeSending('local-new-session');
    useLiveStore.getState().trackSpawn({
      focusHandedOver: false,
      sessionId,
      threadId: 42,
      text: 'start a new session',
      firstSendId: 7,
      workdir: '/work/dir',
      launchOptionIds: [],
      provider: 'claude',
      worktree: null,
      pullRequestNumber: null,
    });
  }

  /**
   * The in-flight chip a new-session submit records before its
   * `POST /api/sends` leaves. Its presence is what tells a `spawn_failed` for
   * an id this client does not track apart from one whose registration is
   * never coming at all — so every "the failure outran the POST" test has to
   * put the client in that window first.
   */
  function beginNewSessionPost() {
    useLiveStore.getState().beginSending({
      id: 'local-new-session',
      target: {
        kind: 'new-session',
        workdir: '/work/dir',
        launchOptionIds: [],
        provider: 'claude',
        worktree: null,
        pullRequestNumber: null,
      },
      text: 'start a new session',
      status: 'sending',
      createdAt: Date.now(),
    });
  }

  it('flips the tracked spawn to failed on its spawn_failed event', () => {
    trackOne();
    useLiveStore.getState().recordLocalSend(
      localSend({ sendId: 7, sessionId: 'sess-spawn-1', threadId: 42 }),
    );

    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });

    const spawn = useLiveStore.getState().spawns[0];
    expect(spawn.status).toBe('failed');
    expect(spawn.text).toBe('start a new session');
    expect(spawn.workdir).toBe('/work/dir');
    // The first send's turn will never end; its tracked twin goes with it.
    expect(useLiveStore.getState().localSends).toEqual({});
  });

  it('marks a spawn the user cancelled, so it is not worded as a failure', () => {
    // The event's own flag is what carries this: a launch the user cancelled
    // (they closed a session that was still starting) reaches the failed
    // session's screen exactly as a broken one does, and the `reason` prose is
    // not something to match on.
    trackOne();

    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: true,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
      reason: 'closed while starting',
    });

    const spawn = useLiveStore.getState().spawns[0];
    expect(spawn.status).toBe('failed');
    expect(spawn.cancelled).toBe(true);
  });

  it('carries the cancelled flag across a failure that outran its POST', () => {
    // The buffered failure has to hand the flag on too, or a cancel that beat
    // the POST response back would be worded as a breakage by the very entry
    // `trackSpawn` registers.
    beginNewSessionPost();
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: true,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
      reason: 'closed while starting',
    });

    trackOne();

    expect(useLiveStore.getState().spawns[0].cancelled).toBe(true);
  });

  it('keeps the failed spawn’s provider and worktree for its Retry', () => {
    // The retry payload is the WHOLE launch configuration, not just the text
    // and the directory: a failed Codex + worktree launch that came back as a
    // Claude session in the plain workdir is a different session than the one
    // the user asked for.
    const worktree = {
      start_point: { kind: 'remote_branch' as const, name: 'develop' },
    };
    useLiveStore.getState().trackSpawn({
      focusHandedOver: false,
      sessionId: 'sess-spawn-codex',
      threadId: 42,
      text: 'start on codex',
      firstSendId: 7,
      workdir: '/work/dir',
      launchOptionIds: [4],
      provider: 'codex',
      worktree,
      pullRequestNumber: null,
    });

    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-codex',
      pane_token: 'pane-1',
    });

    const spawn = useLiveStore.getState().spawns[0];
    expect(spawn.status).toBe('failed');
    expect(spawn.provider).toBe('codex');
    expect(spawn.worktree).toEqual(worktree);
    expect(spawn.launchOptionIds).toEqual([4]);
  });

  it('registers an already-failed spawn when the failure outran its POST', () => {
    // The entry must never be registered `spawning` after its launch has
    // already ended: the workspace would hand focus to a session that is never
    // coming up, and nothing would flip it afterwards — the event that would
    // have is the one that already arrived.
    beginNewSessionPost();
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
      reason: 'git error: worktree add failed',
    });
    trackOne();

    expect(useLiveStore.getState().spawns[0].status).toBe('failed');
  });

  it('releases the tracked spawn when its session registers', () => {
    // The launch bound: the spawn window is over. The workspace focused the
    // session when its send was accepted and its row is listed by now, so the
    // tracked entry has nothing left to do. Only that one entry goes.
    trackOne();
    trackOne('sess-spawn-2');

    useLiveStore.getState().applyEvent({
      kind: 'session_registered',
      session_id: 'sess-spawn-1',
    });

    expect(
      useLiveStore.getState().spawns.map((spawn) => spawn.sessionId),
    ).toEqual(['sess-spawn-2']);
  });

  it('drops a tracked spawn when its session is removed', () => {
    // A failed entry outlives its launch on purpose — it is what that session's
    // Retry reads its launch configuration from — so the session going away is
    // what ends it. The navigator's kebab, another tab and another browser all
    // reach the same removal without passing through that session's screen, and
    // this event is what they have in common.
    trackOne();
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });
    expect(useLiveStore.getState().spawns).toHaveLength(1);

    useLiveStore
      .getState()
      .applyEvent({ kind: 'session_removed', session_id: 'sess-spawn-1' });
    expect(useLiveStore.getState().spawns).toEqual([]);
  });

  it('keeps a failed spawn when a session_registered arrives for it', () => {
    // A `failed` entry is what its session's Retry reads the launch
    // configuration from, and only the user's answer — a retry, or removing the
    // session — drops it. A registration for the same id must never take it
    // away under them.
    trackOne();
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });

    useLiveStore.getState().applyEvent({
      kind: 'session_registered',
      session_id: 'sess-spawn-1',
    });

    expect(useLiveStore.getState().spawns[0].status).toBe('failed');
  });

  it('leaves tracked spawns alone on session_registered for another session', () => {
    trackOne();

    useLiveStore.getState().applyEvent({
      kind: 'session_registered',
      session_id: 'sess-someone-elses',
    });
    expect(useLiveStore.getState().spawns).toHaveLength(1);
  });

  it('leaves tracked spawns alone on spawn_failed for an unknown session', () => {
    trackOne();

    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-someone-elses',
      pane_token: 'pane-x',
    });
    expect(useLiveStore.getState().spawns[0].status).toBe('spawning');
  });

  it('fails a spawn whose spawn_failed event outran its POST response', () => {
    // The live channel and the POST response are independent: the watchdog's
    // broadcast can land before this client processes the response that
    // carries the spawn's ids. The failure must be buffered, not dropped, or
    // the chip spins forever (regression: it was dropped).
    beginNewSessionPost();
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });
    expect(useLiveStore.getState().spawns).toHaveLength(0);

    // The POST resolves: the send is tracked, then the spawn — which lands
    // already failed, with its Retry payload intact and the doomed local send
    // dropped.
    useLiveStore.getState().recordLocalSend(
      localSend({ sendId: 7, sessionId: 'sess-spawn-1', threadId: 42 }),
    );
    trackOne();

    const spawn = useLiveStore.getState().spawns[0];
    expect(spawn.status).toBe('failed');
    expect(spawn.text).toBe('start a new session');
    expect(spawn.workdir).toBe('/work/dir');
    expect(useLiveStore.getState().localSends).toEqual({});
    expect(
      noticeOf(notices(), 'sess-spawn-1', 'spawn_failure_buffered'),
    ).toBeNull();
  });

  it('drops a buffered early failure once that session registers', () => {
    beginNewSessionPost();
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-foreign',
      pane_token: 'pane-x',
    });
    expect(
      noticeOf(notices(), 'sess-foreign', 'spawn_failure_buffered'),
    ).not.toBeNull();

    useLiveStore.getState().applyEvent({
      kind: 'session_registered',
      session_id: 'sess-foreign',
    });
    expect(
      noticeOf(notices(), 'sess-foreign', 'spawn_failure_buffered'),
    ).toBeNull();

    // A later spawn tracked for a clean id starts `spawning` as usual.
    trackOne('sess-foreign');
    expect(useLiveStore.getState().spawns[0].status).toBe('spawning');
  });

  it('does not buffer a duplicate spawn_failed for an already-failed spawn', () => {
    trackOne();
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });
    expect(useLiveStore.getState().spawns[0].status).toBe('failed');
    expect(
      noticeOf(notices(), 'sess-spawn-1', 'spawn_failure_buffered'),
    ).toBeNull();
  });

  it('keeps a failed spawn through unrelated turn events until cleared', () => {
    trackOne();
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });

    // A failed entry is terminal: an unrelated turn ending must not silently
    // remove it. It survives until the user retries or removes the session.
    useLiveStore.getState().applyEvent({
      kind: 'turn_completed',
      session_id: 'sess-1',
      thread_id: 1,
      stop_reason: null,
    });
    expect(useLiveStore.getState().spawns[0].status).toBe('failed');

    useLiveStore.getState().clearSpawn('sess-spawn-1');
    expect(useLiveStore.getState().spawns).toHaveLength(0);
  });

  it('announces an untracked failure in the snackbar', () => {
    // The reload story: the spawn registry lives in memory only, so a browser
    // that reloaded during a slow launch knows nothing about the session it
    // started — and no POST is on its way to teach it. The failed session is
    // listed and has a screen of its own, but nothing on THIS screen would
    // otherwise say a launch just ended, so the snackbar does.
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
      reason: 'git error: worktree add failed',
    });

    expect(
      useNotificationStore
        .getState()
        .notifications.map(({ tone, title, detail }) => ({
          tone,
          title,
          detail,
        })),
    ).toEqual([
      {
        tone: 'error',
        title: 'The session failed to start',
        detail: 'git error: worktree add failed',
      },
    ]);
  });

  it('announces only the first line of a reason that quotes the pane', () => {
    // Since the watchdog began quoting what the pane was showing, a reason can
    // run to a dozen lines of a TUI. The snackbar is a fixed-width box that
    // dismisses itself after a few seconds, so the capture would be a wall of
    // text nobody can read in time — and it has a better home: the failed
    // session's own screen renders the whole reason. Only the headline here,
    // for both tones: a cancel's reason is written by the same server path.
    const captured =
      'The launch did not start within 30 seconds, so Delta gave it up.' +
      '\n\nIts terminal was showing:\n╭─ Do you trust the files in this folder?' +
      '\n│ 1. Yes, proceed';
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
      reason: captured,
    });
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: true,
      session_id: 'sess-spawn-2',
      pane_token: 'pane-2',
      reason: captured,
    });

    expect(
      useNotificationStore.getState().notifications.map(({ detail }) => detail),
    ).toEqual([
      'The launch did not start within 30 seconds, so Delta gave it up.',
      'The launch did not start within 30 seconds, so Delta gave it up.',
    ]);
  });

  it('announces an untracked cancel as information, not as an error', () => {
    // Same path as the untracked failure above (nothing on this screen would
    // otherwise say so), but the user asked for this one: dressing their own
    // action in error colours would tell them it broke something.
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: true,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
      reason: 'closed while starting',
    });

    expect(
      useNotificationStore
        .getState()
        .notifications.map(({ tone, title, detail }) => ({
          tone,
          title,
          detail,
        })),
    ).toEqual([
      {
        tone: 'info',
        title: 'Launch cancelled',
        detail: 'closed while starting',
      },
    ]);
  });

  it('offers to open the session an untracked launch ended in', () => {
    // The snackbar says a launch somewhere ended; the action takes the user to
    // it, for a breakage and a cancel alike — the event names the session.
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    });
    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: true,
      session_id: 'sess-spawn-2',
      pane_token: 'pane-2',
    });

    expect(
      useNotificationStore.getState().notifications.map(({ action }) => action),
    ).toEqual([
      { kind: 'open-session', sessionId: 'sess-spawn-1' },
      { kind: 'open-session', sessionId: 'sess-spawn-2' },
    ]);
  });

  it('announces an untracked failure once, however often it repeats', () => {
    const event = {
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-1',
      pane_token: 'pane-1',
    } as const;
    useLiveStore.getState().applyEvent(event);
    useLiveStore.getState().applyEvent(event);

    expect(useNotificationStore.getState().notifications).toHaveLength(1);
  });

  it('fails each spawn by its own id, leaving the others alone', () => {
    trackOne('sess-spawn-1');
    trackOne('sess-spawn-2');

    useLiveStore.getState().applyEvent({
      kind: 'spawn_failed',
      cancelled: false,
      session_id: 'sess-spawn-2',
      pane_token: 'pane-2',
    });

    const byId = Object.fromEntries(
      useLiveStore.getState().spawns.map((spawn) => [spawn.sessionId, spawn]),
    );
    expect(byId['sess-spawn-1'].status).toBe('spawning');
    expect(byId['sess-spawn-2'].status).toBe('failed');
  });
});
