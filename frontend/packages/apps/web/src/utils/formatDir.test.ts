import { describe, expect, it } from 'vitest';
import { formatDir } from './formatDir';

describe('formatDir', () => {
  it('collapses a /home/<user> prefix to ~', () => {
    expect(formatDir('/home/alice/x')).toBe('~/x');
  });

  it('collapses a /Users/<user> prefix to ~ (macOS)', () => {
    expect(formatDir('/Users/bob/y')).toBe('~/y');
  });

  it('collapses a /root prefix to ~', () => {
    expect(formatDir('/root/z')).toBe('~/z');
  });

  it('leaves a non-home path unchanged', () => {
    expect(formatDir('/var/log/app')).toBe('/var/log/app');
  });

  it('collapses the bare home directory itself to ~', () => {
    // The replacement is anchored to a `/` or end-of-string boundary, so the
    // home directory with no trailing segment collapses to exactly `~`.
    expect(formatDir('/home/alice')).toBe('~');
    expect(formatDir('/root')).toBe('~');
  });

  it('does not collapse a path that merely starts with the same letters', () => {
    // `/homepages` shares a prefix with `/home` but is not a home directory; the
    // boundary anchor must prevent a false match.
    expect(formatDir('/homepages/site')).toBe('/homepages/site');
    expect(formatDir('/rootkit')).toBe('/rootkit');
  });
});
