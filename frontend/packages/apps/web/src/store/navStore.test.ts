import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  DEFAULT_TERMINAL_WIDTH,
  clampTerminalWidth,
  useNavStore,
} from './navStore';

afterEach(() => {
  vi.unstubAllGlobals();
  useNavStore.setState({ terminalWidth: DEFAULT_TERMINAL_WIDTH });
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
