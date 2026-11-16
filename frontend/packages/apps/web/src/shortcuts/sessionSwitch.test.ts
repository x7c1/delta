import { describe, expect, it } from 'vitest';
import { sessionSwitchDirection, sessionSwitchTarget } from './sessionSwitch';

describe('sessionSwitchTarget', () => {
  const ids = ['a', 'b', 'c'];

  it('moves to the next and the previous id in the middle of the list', () => {
    expect(sessionSwitchTarget('b', ids, 'next')).toBe('c');
    expect(sessionSwitchTarget('b', ids, 'previous')).toBe('a');
  });

  it('wraps from the last id to the first, and from the first to the last', () => {
    expect(sessionSwitchTarget('c', ids, 'next')).toBe('a');
    expect(sessionSwitchTarget('a', ids, 'previous')).toBe('c');
  });

  it('moves to the first id forward and the last backward with no focused session', () => {
    expect(sessionSwitchTarget(null, ids, 'next')).toBe('a');
    expect(sessionSwitchTarget(null, ids, 'previous')).toBe('c');
  });

  it('treats a focused id missing from the list as no focus', () => {
    expect(sessionSwitchTarget('gone', ids, 'next')).toBe('a');
    expect(sessionSwitchTarget('gone', ids, 'previous')).toBe('c');
  });

  it('stays on the only id of a one-row list', () => {
    expect(sessionSwitchTarget('a', ['a'], 'next')).toBe('a');
    expect(sessionSwitchTarget('a', ['a'], 'previous')).toBe('a');
    expect(sessionSwitchTarget(null, ['a'], 'next')).toBe('a');
  });

  it('has no target in an empty list', () => {
    expect(sessionSwitchTarget(null, [], 'next')).toBeNull();
    expect(sessionSwitchTarget('a', [], 'previous')).toBeNull();
  });
});

describe('sessionSwitchDirection', () => {
  function press(init: KeyboardEventInit): KeyboardEvent {
    return new KeyboardEvent('keydown', { key: 'Tab', ...init });
  }

  it('reads Ctrl+Tab as next and Ctrl+Shift+Tab as previous', () => {
    expect(sessionSwitchDirection(press({ ctrlKey: true }))).toBe('next');
    expect(
      sessionSwitchDirection(press({ ctrlKey: true, shiftKey: true })),
    ).toBe('previous');
  });

  it('rejects Tab, Shift+Tab, Ctrl+Alt+Tab and Meta+Tab', () => {
    expect(sessionSwitchDirection(press({}))).toBeNull();
    expect(sessionSwitchDirection(press({ shiftKey: true }))).toBeNull();
    expect(
      sessionSwitchDirection(press({ ctrlKey: true, altKey: true })),
    ).toBeNull();
    expect(sessionSwitchDirection(press({ metaKey: true }))).toBeNull();
    expect(
      sessionSwitchDirection(press({ ctrlKey: true, metaKey: true })),
    ).toBeNull();
  });

  it('rejects Ctrl with another key', () => {
    expect(
      sessionSwitchDirection(press({ key: 'q', ctrlKey: true })),
    ).toBeNull();
  });
});
