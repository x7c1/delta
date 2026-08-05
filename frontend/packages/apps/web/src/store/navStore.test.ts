import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  DEFAULT_TERMINAL_WIDTH,
  NAV_STORAGE_KEY,
  NEW_SESSION_FOCUS,
  clampTerminalWidth,
  useNavStore,
} from './navStore';

afterEach(() => {
  vi.unstubAllGlobals();
  useNavStore.setState({
    terminalWidth: DEFAULT_TERMINAL_WIDTH,
    focusedSessionId: null,
    preNewSessionFocus: null,
    activeThreadId: null,
    activeThreadJumpTarget: null,
    settingsOpen: false,
  });
});

describe('clampTerminalWidth', () => {
  it('keeps a value already inside the range', () => {
    expect(clampTerminalWidth(400, 2000)).toBe(400);
  });

  it('clamps below the minimum up to 280px', () => {
    expect(clampTerminalWidth(100, 2000)).toBe(280);
  });

  it('clamps above the maximum down to 720px on a wide viewport', () => {
    expect(clampTerminalWidth(5000, 4000)).toBe(720);
  });

  it('caps at 60% of the viewport when that is the tighter bound', () => {
    // 60% of 1000 = 600, which is below the 720 hard cap.
    expect(clampTerminalWidth(5000, 1000)).toBe(600);
  });

  it('never drops below the minimum even on tiny viewports', () => {
    // 60% of 300 = 180 < 280; the minimum still wins.
    expect(clampTerminalWidth(9999, 300)).toBe(280);
  });
});

describe('navStore.setTerminalWidth', () => {
  it('stores a clamped width using the current viewport', () => {
    vi.stubGlobal('window', { innerWidth: 2000 });
    useNavStore.getState().setTerminalWidth(100);
    expect(useNavStore.getState().terminalWidth).toBe(280);

    useNavStore.getState().setTerminalWidth(450);
    expect(useNavStore.getState().terminalWidth).toBe(450);
  });
});

describe('navStore persistence', () => {
  it('writes the focused session, active thread, and terminal layout to localStorage', () => {
    vi.stubGlobal('window', { innerWidth: 2000 });
    useNavStore.getState().setFocusedSession('sess-9');
    useNavStore.getState().setActiveThread(7);
    useNavStore.getState().setTerminalOpen(true);
    useNavStore.getState().setTerminalWidth(500);

    const raw = localStorage.getItem(NAV_STORAGE_KEY);
    expect(raw).not.toBeNull();
    const persisted = JSON.parse(raw as string).state;
    expect(persisted.focusedSessionId).toBe('sess-9');
    expect(persisted.activeThreadId).toBe(7);
    expect(persisted.terminalOpen).toBe(true);
    expect(persisted.terminalWidth).toBe(500);
  });
});

describe('navStore.setFocusedSession', () => {
  it('clears the active thread when switching to a different session', () => {
    useNavStore.getState().setFocusedSession('sess-a');
    useNavStore.getState().setActiveThread(7);
    useNavStore.getState().setFocusedSession('sess-b');
    expect(useNavStore.getState().activeThreadId).toBeNull();
  });

  it('leaves the active thread intact when re-focusing the same session', () => {
    useNavStore.getState().setFocusedSession('sess-a');
    useNavStore.getState().setActiveThread(7);
    useNavStore.getState().setFocusedSession('sess-a');
    expect(useNavStore.getState().activeThreadId).toBe(7);
  });

  it('is a true no-op when re-focusing the same session with settings open', () => {
    // Re-asserting an unchanged focus is not navigation, so it carries none of
    // navigation's side effects — not even while the settings overlay is up.
    useNavStore.getState().setFocusedSession('sess-a');
    useNavStore.getState().setActiveThread(7);
    useNavStore.getState().openSettings();

    useNavStore.getState().setFocusedSession('sess-a');

    expect(useNavStore.getState().activeThreadId).toBe(7);
    expect(useNavStore.getState().settingsOpen).toBe(true);
  });
});

describe('navStore.reconcileFocusedSession', () => {
  it('moves focus and clears the active thread like a navigation', () => {
    useNavStore.getState().setFocusedSession('sess-a');
    useNavStore.getState().setActiveThread(7);

    useNavStore.getState().reconcileFocusedSession('sess-b');

    expect(useNavStore.getState().focusedSessionId).toBe('sess-b');
    expect(useNavStore.getState().activeThreadId).toBeNull();
  });

  it('leaves the settings overlay open', () => {
    // The workspace reconciles focus asynchronously (a spawn registering, a
    // stale persisted focus resolving). That is the app catching up with the
    // server, never the user navigating, so it must not dismiss a dialog the
    // user opened in the meantime.
    useNavStore.getState().openSettings();

    useNavStore.getState().reconcileFocusedSession('sess-a');

    expect(useNavStore.getState().focusedSessionId).toBe('sess-a');
    expect(useNavStore.getState().settingsOpen).toBe(true);
  });
});

describe('navStore.startNewSession', () => {
  it('stashes the current focus and enters the new-session state', () => {
    useNavStore.getState().setFocusedSession('sess-1');
    useNavStore.getState().setActiveThread(7);

    useNavStore.getState().startNewSession();

    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
    expect(useNavStore.getState().preNewSessionFocus).toBe('sess-1');
    expect(useNavStore.getState().activeThreadId).toBeNull();
  });

  it('does not overwrite the stashed focus when already in new-session', () => {
    useNavStore.getState().setFocusedSession('sess-1');
    useNavStore.getState().startNewSession();
    // A second entry (e.g. clicking "New" again) must keep the real previous
    // focus, not stash the sentinel.
    useNavStore.getState().startNewSession();

    expect(useNavStore.getState().preNewSessionFocus).toBe('sess-1');
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
  });
});

describe('navStore.cancelNewSession', () => {
  it('restores a real previous session and clears the stash + active thread', () => {
    useNavStore.getState().setFocusedSession('sess-1');
    useNavStore.getState().startNewSession();
    useNavStore.getState().setActiveThread(9);

    useNavStore.getState().cancelNewSession();

    expect(useNavStore.getState().focusedSessionId).toBe('sess-1');
    expect(useNavStore.getState().preNewSessionFocus).toBeNull();
    expect(useNavStore.getState().activeThreadId).toBeNull();
  });

  it('is a no-op when the stashed focus is null (empty initial screen)', () => {
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: null,
    });

    useNavStore.getState().cancelNewSession();

    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
    expect(useNavStore.getState().preNewSessionFocus).toBeNull();
  });

  it('is a no-op when the stashed focus is the new-session sentinel', () => {
    useNavStore.setState({
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: NEW_SESSION_FOCUS,
    });

    useNavStore.getState().cancelNewSession();

    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
    expect(useNavStore.getState().preNewSessionFocus).toBe(NEW_SESSION_FOCUS);
  });
});

describe('navStore settings overlay', () => {
  it('opens and closes the settings overlay', () => {
    expect(useNavStore.getState().settingsOpen).toBe(false);
    useNavStore.getState().openSettings();
    expect(useNavStore.getState().settingsOpen).toBe(true);
    useNavStore.getState().closeSettings();
    expect(useNavStore.getState().settingsOpen).toBe(false);
  });

  it('closes the settings overlay when a session is focused', () => {
    useNavStore.getState().openSettings();
    useNavStore.getState().setFocusedSession('sess-a');
    expect(useNavStore.getState().settingsOpen).toBe(false);
    expect(useNavStore.getState().focusedSessionId).toBe('sess-a');
  });

  it('closes the settings overlay when a new session starts', () => {
    useNavStore.getState().openSettings();
    useNavStore.getState().startNewSession();
    expect(useNavStore.getState().settingsOpen).toBe(false);
    expect(useNavStore.getState().focusedSessionId).toBe(NEW_SESSION_FOCUS);
  });
});

describe('navStore persistence (new-session intent)', () => {
  it('never persists preNewSessionFocus', () => {
    vi.stubGlobal('window', { innerWidth: 2000 });
    useNavStore.getState().setFocusedSession('sess-1');
    useNavStore.getState().startNewSession();

    const raw = localStorage.getItem(NAV_STORAGE_KEY);
    expect(raw).not.toBeNull();
    const persisted = JSON.parse(raw as string).state;
    expect(persisted).not.toHaveProperty('preNewSessionFocus');
  });
});

describe('navStore active-thread jump intent', () => {
  it('setActiveThreadWithJumpTarget switches the thread and records the intent atomically', () => {
    useNavStore.getState().setActiveThreadWithJumpTarget(5, 'uuid-x');
    expect(useNavStore.getState().activeThreadId).toBe(5);
    expect(useNavStore.getState().activeThreadJumpTarget).toEqual({
      threadId: 5,
      targetUuid: 'uuid-x',
    });
  });

  it('setActiveThread clears any pending jump intent (plain navigation)', () => {
    useNavStore.getState().setActiveThreadWithJumpTarget(5, 'uuid-x');
    useNavStore.getState().setActiveThread(6);
    expect(useNavStore.getState().activeThreadId).toBe(6);
    expect(useNavStore.getState().activeThreadJumpTarget).toBeNull();
  });

  it('clearActiveThreadJumpTarget with no argument clears unconditionally', () => {
    useNavStore.getState().setActiveThreadWithJumpTarget(5, 'uuid-x');
    useNavStore.getState().clearActiveThreadJumpTarget();
    expect(useNavStore.getState().activeThreadJumpTarget).toBeNull();
  });

  it('clearActiveThreadJumpTarget only clears when the expected uuid still matches (no clobber of a newer jump)', () => {
    useNavStore.getState().setActiveThreadWithJumpTarget(5, 'first');
    // A newer jump replaces the intent before the earlier one settles.
    useNavStore.getState().setActiveThreadWithJumpTarget(6, 'second');
    // The earlier jump's settle callback must NOT clear the newer intent.
    useNavStore.getState().clearActiveThreadJumpTarget('first');
    expect(useNavStore.getState().activeThreadJumpTarget).toEqual({
      threadId: 6,
      targetUuid: 'second',
    });
    // The newer jump's own settle clears it.
    useNavStore.getState().clearActiveThreadJumpTarget('second');
    expect(useNavStore.getState().activeThreadJumpTarget).toBeNull();
  });

  it('does not persist the jump intent to localStorage', () => {
    vi.stubGlobal('window', { innerWidth: 2000 });
    useNavStore.getState().setActiveThreadWithJumpTarget(5, 'uuid-x');
    const raw = localStorage.getItem(NAV_STORAGE_KEY);
    expect(raw).not.toBeNull();
    const persisted = JSON.parse(raw as string).state;
    expect(persisted).not.toHaveProperty('activeThreadJumpTarget');
  });
});
