/**
 * Ctrl+Tab / Ctrl+Shift+Tab in the desktop shell: moving the focus through the
 * rows in the order the navigator renders them.
 *
 * Shared fixtures live in NavigatorPane.testkit.tsx.
 */
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { act } from '@testing-library/react';
import type { SessionListItem } from '@delta/wire-gen';
import { LINUX_SHELL, MACOS_SHELL } from '../../shell';
import { useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';
import { makeItem, renderPane, server } from './NavigatorPane.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
afterEach(() => {
  server.resetHandlers();
  delete document.documentElement.dataset.shell;
});
afterAll(() => server.close());

const alpha = makeItem('alpha', 101);
const beta = makeItem('beta', 102);
const gamma = makeItem('gamma', 103);

/** Dispatch a Ctrl+Tab (or, with `shift`, Ctrl+Shift+Tab) keydown on `target`. */
function pressCtrlTab(
  target: EventTarget = document.body,
  shift = false,
): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    key: 'Tab',
    code: 'Tab',
    ctrlKey: true,
    shiftKey: shift,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    target.dispatchEvent(event);
  });
  return event;
}

function focused() {
  return useNavStore.getState().focusedSessionId;
}

beforeEach(() => {
  useLiveStore.setState({
    connection: 'open',
    notices: {},
    runningThreads: {},
    unread: {},
    spawns: [],
  });
  useNavStore.setState({ focusedSessionId: null, activeThreadId: null });
});

describe.each([
  ['Linux', LINUX_SHELL],
  ['macOS', MACOS_SHELL],
])('NavigatorPane session switch in the %s shell', (_name, shell) => {
  beforeEach(() => {
    document.documentElement.dataset.shell = shell;
  });

  it('focuses the next row on Ctrl+Tab and the previous one on Ctrl+Shift+Tab', () => {
    useNavStore.setState({ focusedSessionId: 'beta' });
    renderPane([alpha, beta, gamma]);

    const forward = pressCtrlTab();
    expect(focused()).toBe('gamma');
    expect(forward.defaultPrevented).toBe(true);

    const backward = pressCtrlTab(document.body, true);
    expect(focused()).toBe('beta');
    expect(backward.defaultPrevented).toBe(true);

    pressCtrlTab(document.body, true);
    expect(focused()).toBe('alpha');
  });

  it('wraps around at both ends', () => {
    useNavStore.setState({ focusedSessionId: 'gamma' });
    renderPane([alpha, beta, gamma]);

    pressCtrlTab();
    expect(focused()).toBe('alpha');
    pressCtrlTab(document.body, true);
    expect(focused()).toBe('gamma');
  });

  it('consumes the key with nothing to move to in an empty list', () => {
    renderPane([]);

    const event = pressCtrlTab();
    expect(focused()).toBeNull();
    expect(event.defaultPrevented).toBe(true);
  });
});

describe('NavigatorPane session switch order', () => {
  beforeEach(() => {
    document.documentElement.dataset.shell = LINUX_SHELL;
  });

  it('follows the rendered order, with the tracked launch pinned above the server order', () => {
    const failed: SessionListItem = { ...makeItem('failed', 999), open: false };
    failed.session = { ...failed.session, status: 'failed' };
    useLiveStore.setState({
      spawns: [
        {
          sessionId: 'failed',
          threadId: 999,
          text: 'first message',
          firstSendId: 1,
          workdir: '/projects/app',
          launchOptionIds: [],
          provider: 'claude',
          worktree: null,
          pullRequestNumber: null,
          status: 'failed',
        },
      ],
    });
    // The server lists the failed launch last; the navigator renders it first.
    renderPane([alpha, beta, failed]);

    pressCtrlTab();
    expect(focused()).toBe('failed');
    pressCtrlTab();
    expect(focused()).toBe('alpha');
    pressCtrlTab(document.body, true);
    pressCtrlTab(document.body, true);
    expect(focused()).toBe('beta');
  });

  it('takes the key before an element inside the page sees it', () => {
    useNavStore.setState({ focusedSessionId: 'alpha' });
    renderPane([alpha, beta, gamma]);
    // Stands in for xterm's helper textarea or the composer.
    const textarea = document.createElement('textarea');
    document.body.appendChild(textarea);
    let reached = 0;
    textarea.addEventListener('keydown', () => {
      reached += 1;
    });
    try {
      textarea.focus();
      const event = pressCtrlTab(textarea);

      expect(focused()).toBe('beta');
      expect(event.defaultPrevented).toBe(true);
      expect(reached).toBe(0);
    } finally {
      textarea.remove();
    }
  });
});

describe('NavigatorPane session switch in a browser', () => {
  it('leaves Ctrl+Tab alone without a desktop shell', () => {
    useNavStore.setState({ focusedSessionId: 'alpha' });
    renderPane([alpha, beta, gamma]);

    const event = pressCtrlTab();
    expect(focused()).toBe('alpha');
    expect(event.defaultPrevented).toBe(false);
  });
});
