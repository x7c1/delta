import { describe, expect, it } from 'vitest';
import {
  MAIN_THREAD_DISPLAY_NAME,
  emptyTitleFallback,
  threadDisplayName,
  threadTooltip,
} from './thread-name';

describe('threadDisplayName', () => {
  it('returns the wire title verbatim for a normal subthread', () => {
    expect(threadDisplayName({ id: 2, title: 'branch one' })).toBe('branch one');
  });

  it(`returns "${MAIN_THREAD_DISPLAY_NAME}" when the caller flags it as main`, () => {
    // The main thread's wire title is typically the session prompt itself
    // (long and not useful as a label). The conventional name wins instead.
    expect(
      threadDisplayName(
        { id: 1, title: 'Investigate the staging migration failure end to end' },
        { isMain: true },
      ),
    ).toBe(MAIN_THREAD_DISPLAY_NAME);
  });

  it('falls back to `thread <id>` when the title is empty after trimming', () => {
    expect(threadDisplayName({ id: 42, title: '' })).toBe('thread 42');
    expect(threadDisplayName({ id: 42, title: '   ' })).toBe('thread 42');
  });

  it('preserves multi-line and very long titles unchanged — truncation is the caller’s job', () => {
    const long = `line one\nline two ${'x'.repeat(200)}`;
    expect(threadDisplayName({ id: 3, title: long })).toBe(long);
  });
});

describe('threadTooltip', () => {
  it('mirrors threadDisplayName for the main thread', () => {
    expect(
      threadTooltip({ id: 1, title: 'a long prompt' }, { isMain: true }),
    ).toBe(MAIN_THREAD_DISPLAY_NAME);
  });

  it('returns the trimmed full title for a subthread', () => {
    expect(threadTooltip({ id: 2, title: '  branch one  ' })).toBe('branch one');
  });

  it('falls back to `thread <id>` when the title is empty', () => {
    expect(threadTooltip({ id: 9, title: '' })).toBe('thread 9');
  });
});

describe('emptyTitleFallback', () => {
  it('formats a stable id-based label', () => {
    expect(emptyTitleFallback(7)).toBe('thread 7');
  });
});
