import { describe, expect, it } from 'vitest';
import { pathTail } from './pathTail';

describe('pathTail', () => {
  it('keeps the last two segments of a deep path', () => {
    expect(pathTail('/a/b/c/d/e')).toBe('d/e');
  });

  it('returns the only segment when the path is shallow', () => {
    expect(pathTail('/a')).toBe('a');
  });

  it('returns an empty string for the root path', () => {
    expect(pathTail('/')).toBe('');
  });

  it('returns an empty string for an empty path', () => {
    expect(pathTail('')).toBe('');
  });

  it('ignores a trailing slash', () => {
    expect(pathTail('/a/b/')).toBe('a/b');
  });
});
