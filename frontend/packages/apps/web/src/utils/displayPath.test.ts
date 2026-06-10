import { describe, expect, it } from 'vitest';
import { displayPath } from './displayPath';

describe('displayPath', () => {
  it('collapses an exact home match to `~`', () => {
    expect(displayPath('/home/alice', '/home/alice')).toBe('~');
  });

  it('collapses a path under home to `~/…`', () => {
    expect(displayPath('/home/alice/projects/x', '/home/alice')).toBe(
      '~/projects/x',
    );
  });

  it('leaves a sibling whose name only shares a prefix unchanged', () => {
    // `/home/developer` is not under `/home/dev` — the boundary `/` matters.
    expect(displayPath('/home/developer', '/home/dev')).toBe('/home/developer');
  });

  it('leaves a path not under home unchanged', () => {
    expect(displayPath('/var/tmp', '/home/dev')).toBe('/var/tmp');
  });

  it('leaves the path unchanged when home is unknown', () => {
    expect(displayPath('/home/dev/projects', null)).toBe('/home/dev/projects');
  });
});
