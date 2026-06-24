import { describe, expect, it } from 'vitest';
import { cn } from './cn';

describe('cn', () => {
  it('joins non-conflicting class fragments verbatim', () => {
    expect(cn('flex items-center', 'gap-2')).toBe('flex items-center gap-2');
  });

  it('drops falsy fragments before merging', () => {
    expect(cn('px-2', false, 'px-4', undefined)).toBe('px-4');
    expect(cn('px-2', null, 'py-1')).toBe('px-2 py-1');
  });

  // The Settings dialog regression: a consumer's `max-w-4xl` was being beaten
  // by the Dialog primitive's `max-w-md` default under plain concatenation.
  it('lets a later max-width override an earlier one', () => {
    expect(cn('max-w-md', 'max-w-4xl')).toBe('max-w-4xl');
  });

  it('dedups conflicting text color utilities to the last one passed', () => {
    expect(cn('text-slate-500', 'text-slate-700')).toBe('text-slate-700');
  });
});
