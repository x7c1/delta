import { describe, expect, it } from 'vitest';
import { displayBranch } from './display-branch';

describe('displayBranch', () => {
  it('shortens a delta-managed UUID v7 branch to its first 8 hex chars', () => {
    expect(
      displayBranch('delta-019ef8ff-76aa-7870-a0dd-3a5856d28d79'),
    ).toBe('019ef8ff');
  });

  it('shortens a delta-managed UUID v4 branch the same way', () => {
    // The pattern is hex-only with the canonical 8-4-4-4-12 grouping, so v4
    // and v7 ids are indistinguishable at the regex level — both shorten.
    expect(
      displayBranch('delta-3b9aa2bd-7f0c-4a1b-9d4e-2c5f7a8b1234'),
    ).toBe('3b9aa2bd');
  });

  it('accepts upper-case hex (defensive — delta itself emits lower-case)', () => {
    expect(
      displayBranch('delta-019EF8FF-76AA-7870-A0DD-3A5856D28D79'),
    ).toBe('019EF8FF');
  });

  it('leaves `main` untouched', () => {
    expect(displayBranch('main')).toBe('main');
  });

  it('leaves a user-created feature branch untouched', () => {
    expect(displayBranch('feat/some-thing')).toBe('feat/some-thing');
  });

  it('leaves a delta- prefix that is NOT followed by a UUID untouched', () => {
    // `delta-` is a perfectly legal prefix for a user-named branch; only the
    // exact `delta-<uuid>` shape is treated as machine-generated.
    expect(displayBranch('delta-experiment')).toBe('delta-experiment');
  });

  it('leaves a name that contains `delta-<uuid>` mid-string untouched', () => {
    // The regex is anchored, so a substring match does not trigger.
    expect(
      displayBranch('prefix/delta-019ef8ff-76aa-7870-a0dd-3a5856d28d79'),
    ).toBe('prefix/delta-019ef8ff-76aa-7870-a0dd-3a5856d28d79');
  });

  it('leaves a delta- UUID with trailing whitespace untouched', () => {
    // The regex is anchored at both ends; surrounding whitespace breaks the
    // match. We deliberately do NOT trim — silently mutating the caller's
    // string would hide upstream bugs.
    const padded = 'delta-019ef8ff-76aa-7870-a0dd-3a5856d28d79 ';
    expect(displayBranch(padded)).toBe(padded);
  });

  it('leaves an already-shortened 8-char hex untouched', () => {
    // A second pass through the helper is a no-op — important for any caller
    // that might double-wrap (e.g. a memoised selector).
    expect(displayBranch('019ef8ff')).toBe('019ef8ff');
  });

  it('leaves the empty string untouched', () => {
    expect(displayBranch('')).toBe('');
  });
});
