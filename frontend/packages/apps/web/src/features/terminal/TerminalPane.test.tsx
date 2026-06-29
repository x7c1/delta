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

vi.mock('@delta/api-client', async () => {
  const actual =
    await vi.importActual<typeof import('@delta/api-client')>('@delta/api-client');
  return {
    ...actual,
    connectPty: () => ({
      close: () => {},
      send: () => {},
      resize: () => {},
    }),
  };
});

// Resolve the canvas background from `<html data-theme="…">` so the bridge
// effect can be observed flipping the value as the active theme changes.
vi.mock('../../theme', () => ({
  terminalBackground: () =>
    document.documentElement.dataset.theme === 'dark' ? '#000000' : '#ffffff',
  terminalFontFamily: () => 'monospace',
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
  return <TerminalPane sessionId={sessionId} attachable={true} />;
}

describe('TerminalPane xterm theme bridge', () => {
  beforeEach(() => {
    fakeTerminals.length = 0;
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
