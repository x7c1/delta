import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import { act, render } from '@testing-library/react';
import type { SessionId } from '@delta/model';
import { ThemeProvider, useThemeContext } from '../../hooks/themeContext';
import {
  SYSTEM_PREFERENCE,
  THEME_PREFERENCE_STORAGE_KEY,
  type ThemePreference,
} from '../../hooks/useTheme';

/**
 * Tests for the xterm live-update bridge in {@link TerminalPane}. xterm's
 * renderer captures `theme.background` once at construction, so a theme flip
 * after the terminal is open must reach the live instance via `options.theme`
 * for the canvas to repaint. This file mocks xterm (and its PTY socket) so
 * the bridge can be exercised in jsdom without needing a real WebGL/canvas
 * pipeline; the focus is the effect that fans the new background out to every
 * pane entry, not xterm's internal rendering.
 */

interface FakeTerminal {
  options: { theme?: { background?: string } };
  rows: number;
  cols: number;
  unicode: { activeVersion: string };
}

const fakeTerminals: FakeTerminal[] = [];

vi.mock('@xterm/xterm', () => {
  class Terminal implements FakeTerminal {
    options: { theme?: { background?: string } };
    rows = 24;
    cols = 80;
    unicode = { activeVersion: '6' };
    constructor(opts: { theme?: { background?: string } }) {
      this.options = { ...opts };
      fakeTerminals.push(this);
    }
    loadAddon(): void {}
    open(): void {}
    write(): void {}
    onData(): void {}
    onResize(): void {}
    dispose(): void {}
    refresh(): void {}
  }
  return { Terminal };
});

vi.mock('@xterm/addon-fit', () => {
  class FitAddon {
    fit(): void {}
  }
  return { FitAddon };
});

vi.mock('@xterm/addon-unicode11', () => {
  class Unicode11Addon {}
  return { Unicode11Addon };
});

// A spy for `connectPty` so tests can assert whether the PTY bridge was opened,
// and a count of bridge closes per session so they can assert it was dropped.
// Hoisted so both are defined before the (hoisted) `vi.mock` factory
// references them.
const { connectPtyMock, ptyCloses } = vi.hoisted(() => {
  const ptyCloses = new Map<string, number>();
  return {
    ptyCloses,
    connectPtyMock: vi.fn((options: { sessionId: string }) => ({
      close: () => {
        ptyCloses.set(
          options.sessionId,
          (ptyCloses.get(options.sessionId) ?? 0) + 1,
        );
      },
      send: () => {},
      resize: () => {},
    })),
  };
});

vi.mock('@delta/api-client', async () => {
  const actual =
    await vi.importActual<typeof import('@delta/api-client')>('@delta/api-client');
  return {
    ...actual,
    connectPty: connectPtyMock,
  };
});

// Resolve the canvas background from `<html data-theme="…">` so the bridge
// effect can be observed flipping the value as the active theme changes.
vi.mock('../../theme', () => ({
  terminalBackground: () =>
    document.documentElement.dataset.theme === 'dark' ? '#000000' : '#ffffff',
  terminalFontFamily: () => 'monospace',
  terminalFontSize: () => 14,
}));

// Force non-mock mode so TerminalPane wires up an xterm instance.
vi.mock('../../config', () => ({
  isMockMode: () => false,
  wsUrl: () => 'ws://localhost/pty',
}));

// Import after the mocks above so TerminalPane resolves them.
import { TerminalPane } from './TerminalPane';

function installMatchMediaStub(prefersDark: boolean) {
  const mql = {
    matches: prefersDark,
    media: '(prefers-color-scheme: dark)',
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  };
  vi.stubGlobal('matchMedia', () => mql);
}

let capturedSetPreference: (next: ThemePreference) => void = () => {};

function ThemeHarness({ sessionId }: { sessionId: SessionId }) {
  const { setPreference } = useThemeContext();
  capturedSetPreference = setPreference;
  return <TerminalPane sessionId={sessionId} paneState="open" hasTerminal />;
}

describe('TerminalPane xterm theme bridge', () => {
  beforeEach(() => {
    fakeTerminals.length = 0;
    connectPtyMock.mockClear();
    capturedSetPreference = () => {};
    localStorage.removeItem(THEME_PREFERENCE_STORAGE_KEY);
    delete document.documentElement.dataset.theme;
    installMatchMediaStub(false);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('repaints the live xterm background when the theme is flipped', () => {
    render(
      <ThemeProvider>
        <ThemeHarness sessionId={'s1' as SessionId} />
      </ThemeProvider>,
    );

    // The terminal is constructed once on mount; under the matchMedia stub
    // the resolved theme is 'light' and the (mocked) terminalBackground()
    // returns the light hex.
    expect(fakeTerminals).toHaveLength(1);
    expect(fakeTerminals[0].options.theme?.background).toBe('#ffffff');

    // Flipping the preference must drive the bridge effect, which reassigns
    // `options.theme` on every live entry — not just call `setPreference`.
    act(() => {
      capturedSetPreference('dark');
    });

    expect(document.documentElement.dataset.theme).toBe('dark');
    expect(fakeTerminals[0].options.theme?.background).toBe('#000000');
  });

  it('tracks the OS preference when set to System', () => {
    // Start on SYSTEM with prefers-dark = false → light.
    render(
      <ThemeProvider>
        <ThemeHarness sessionId={'s1' as SessionId} />
      </ThemeProvider>,
    );
    expect(fakeTerminals[0].options.theme?.background).toBe('#ffffff');

    // Toggling to an explicit dark pick and back to SYSTEM should leave the
    // background on the OS-driven resolution (still light here).
    act(() => {
      capturedSetPreference('dark');
    });
    expect(fakeTerminals[0].options.theme?.background).toBe('#000000');

    act(() => {
      capturedSetPreference(SYSTEM_PREFERENCE);
    });
    expect(document.documentElement.dataset.theme).toBe('light');
    expect(fakeTerminals[0].options.theme?.background).toBe('#ffffff');
  });
});

describe('TerminalPane attaching to a session that is still starting', () => {
  beforeEach(() => {
    fakeTerminals.length = 0;
    connectPtyMock.mockClear();
    installMatchMediaStub(false);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('attaches to a spawn whose pane is up but has not bound', () => {
    // The state this pane exists for: the launch is running in its pane and no
    // hook has arrived, which is where a workspace-trust dialog sits. The
    // bridge must open, because answering it is only possible from here.
    render(
      <ThemeProvider>
        <TerminalPane
          sessionId={'s1' as SessionId}
          paneState="starting"
          hasTerminal={true}
        />
      </ThemeProvider>,
    );

    expect(fakeTerminals).toHaveLength(1);
    expect(connectPtyMock).toHaveBeenCalledTimes(1);
  });

  it('says a starting session is starting, and never that it is closed', () => {
    // The wording matters as much as the attach: a launch still being prepared
    // has no pane to attach to, and telling the user to resume a session that
    // was never closed asks for something they cannot do.
    const { getByText, queryByText } = render(
      <ThemeProvider>
        <TerminalPane
          sessionId={'s1' as SessionId}
          paneState="preparing"
          hasTerminal={true}
        />
      </ThemeProvider>,
    );

    expect(connectPtyMock).not.toHaveBeenCalled();
    expect(getByText(/still starting up/i)).toBeTruthy();
    expect(queryByText(/This session is closed/i)).toBeNull();
  });

  it('says a just-launched session is starting before its id reaches the pane', () => {
    // Focused off its tracked launch in the beat before its row is listed: the
    // pane has no session id yet, but inviting the user to start a session
    // would contradict the Send they just made.
    const { getByText, queryByText } = render(
      <ThemeProvider>
        <TerminalPane sessionId={null} paneState="preparing" hasTerminal={true} />
      </ThemeProvider>,
    );

    expect(connectPtyMock).not.toHaveBeenCalled();
    expect(getByText(/still starting up/i)).toBeTruthy();
    expect(queryByText(/Start a session/i)).toBeNull();
  });

  it('does not attach to a session whose launch failed', () => {
    const { getByText } = render(
      <ThemeProvider>
        <TerminalPane
          sessionId={'s1' as SessionId}
          paneState="failed"
          hasTerminal={true}
        />
      </ThemeProvider>,
    );

    expect(connectPtyMock).not.toHaveBeenCalled();
    expect(getByText(/never started/i)).toBeTruthy();
  });

  it('still tells a closed session to resume', () => {
    const { getByText } = render(
      <ThemeProvider>
        <TerminalPane
          sessionId={'s1' as SessionId}
          paneState="closed"
          hasTerminal={true}
        />
      </ThemeProvider>,
    );

    expect(connectPtyMock).not.toHaveBeenCalled();
    expect(getByText(/This session is closed/i)).toBeTruthy();
  });
});

describe('TerminalPane capability gate on the PTY bridge', () => {
  beforeEach(() => {
    fakeTerminals.length = 0;
    connectPtyMock.mockClear();
    installMatchMediaStub(false);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('opens the PTY bridge for an open session whose provider has a terminal', () => {
    // Claude's case: a bound session whose provider has a terminal.
    // The pane builds its xterm instance and connects the `/pty` bridge — the
    // behaviour must stay byte-identical to before the Codex gate landed.
    render(
      <ThemeProvider>
        <TerminalPane
          sessionId={'s1' as SessionId}
          paneState="open"
          hasTerminal={true}
        />
      </ThemeProvider>,
    );

    expect(fakeTerminals).toHaveLength(1);
    expect(connectPtyMock).toHaveBeenCalledTimes(1);
  });

  it('never opens the PTY bridge for a terminal-less provider', () => {
    // Codex's case: an open session, but its provider reports no
    // terminal. The capability gate must be authoritative for the connect — no
    // xterm is built and, crucially, no `/pty` websocket is opened (which would
    // trip the backend's "no attachable pane" warning).
    render(
      <ThemeProvider>
        <TerminalPane
          sessionId={'s1' as SessionId}
          paneState="open"
          hasTerminal={false}
        />
      </ThemeProvider>,
    );

    expect(fakeTerminals).toHaveLength(0);
    expect(connectPtyMock).not.toHaveBeenCalled();
  });

  it('never opens the PTY bridge while the pane is hidden', () => {
    // The workspace keeps the pane mounted while the focused session's terminal
    // is closed; that must not attach the session nobody asked to see.
    render(
      <ThemeProvider>
        <TerminalPane
          sessionId={'s1' as SessionId}
          paneState="open"
          hasTerminal
          hidden
        />
      </ThemeProvider>,
    );

    expect(fakeTerminals).toHaveLength(0);
    expect(connectPtyMock).not.toHaveBeenCalled();
  });
});

describe('TerminalPane holding a bridge on an unfocused session', () => {
  beforeEach(() => {
    fakeTerminals.length = 0;
    ptyCloses.clear();
    connectPtyMock.mockClear();
    installMatchMediaStub(false);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  function pane(
    sessionId: string | null,
    paneState: 'open' | 'starting',
    hidden = false,
  ) {
    return (
      <ThemeProvider>
        <TerminalPane
          sessionId={sessionId as SessionId | null}
          paneState={paneState}
          hasTerminal
          hidden={hidden}
        />
      </ThemeProvider>
    );
  }

  it('keeps a bound session attached through a session whose terminal is closed', () => {
    // The workspace hides the pane (rather than unmounting it) while the
    // focused session's terminal is closed, so A's bridge and its xterm node
    // survive the trip through B.
    const { container, rerender } = render(pane('s1', 'open'));
    const entryEl = container.querySelector('.absolute.inset-0');
    expect(entryEl).not.toBeNull();

    rerender(pane('s2', 'open', true));
    rerender(pane('s1', 'open'));

    expect(connectPtyMock).toHaveBeenCalledTimes(1);
    expect(ptyCloses.get('s1')).toBeUndefined();
    expect(container.querySelector('.absolute.inset-0')).toBe(entryEl);
    expect((entryEl as HTMLElement).style.display).toBe('block');
  });

  it('drops a starting session’s bridge when its pane is hidden', () => {
    // Off screen is off screen: a hidden starting entry would count as someone
    // watching the launch.
    const { rerender } = render(pane('s1', 'starting'));
    rerender(pane('s1', 'starting', true));
    expect(ptyCloses.get('s1')).toBe(1);
  });

  it('drops the focused session’s bridge when its own terminal is closed, keeping the others', () => {
    const { rerender } = render(pane('s1', 'open'));
    rerender(pane('s2', 'open'));
    rerender(pane('s2', 'open', true));

    expect(ptyCloses.get('s2')).toBe(1);
    expect(ptyCloses.get('s1')).toBeUndefined();
  });

  it('keeps a bound session attached while another is focused', () => {
    // Detaching a bound pane puts a stray blank line into Claude's input, so a
    // bound session's bridge outlives its focus.
    const { rerender } = render(pane('s1', 'open'));
    rerender(pane('s2', 'open'));
    rerender(pane('s1', 'open'));

    expect(ptyCloses.get('s1')).toBeUndefined();
    // No rebuild on the way back: one bridge per session.
    expect(connectPtyMock).toHaveBeenCalledTimes(2);
  });

  it('drops a starting session’s bridge when it loses focus, and rebuilds it on return', () => {
    // An attached starting pane is never reaped by the launch watchdog, so a
    // bridge kept hidden would count as someone watching when nobody is.
    const { rerender } = render(pane('s1', 'starting'));
    rerender(pane('s2', 'open'));
    expect(ptyCloses.get('s1')).toBe(1);

    rerender(pane('s1', 'starting'));
    expect(connectPtyMock).toHaveBeenCalledTimes(3);
    // The bound session left behind is kept.
    expect(ptyCloses.get('s2')).toBeUndefined();
  });

  it('drops a starting session’s bridge when the new-session screen takes focus', () => {
    const { rerender } = render(pane('s1', 'starting'));
    rerender(pane(null, 'open'));
    expect(ptyCloses.get('s1')).toBe(1);
  });

  it('keeps a session attached across its bind, then past its focus', () => {
    // The bind itself never rebuilds the entry (the user may be mid-way through
    // answering the prompt they attached for), and once bound it is held like
    // any open session.
    const { rerender } = render(pane('s1', 'starting'));
    rerender(pane('s1', 'open'));
    rerender(pane('s2', 'open'));

    expect(ptyCloses.get('s1')).toBeUndefined();
    expect(connectPtyMock).toHaveBeenCalledTimes(2);
  });
});
