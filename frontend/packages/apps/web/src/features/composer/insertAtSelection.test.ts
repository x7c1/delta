import { describe, expect, it } from 'vitest';
import { insertAtSelection } from './insertAtSelection';

/**
 * The caret arithmetic behind "insert a prompt template at the cursor". The
 * promise these pin down is that the template's text reaches the draft
 * byte-for-byte — no separator is invented at either end — and that the caret
 * always lands immediately after it, so typing continues where the user
 * expects.
 */
describe('insertAtSelection', () => {
  it('inserts at the start of a draft', () => {
    expect(insertAtSelection('world', 0, 0, 'hello ')).toEqual({
      next: 'hello world',
      caret: 6,
    });
  });

  it('inserts in the middle of a draft', () => {
    expect(insertAtSelection('ab', 1, 1, 'XY')).toEqual({
      next: 'aXYb',
      caret: 3,
    });
  });

  it('inserts at the end of a draft', () => {
    expect(insertAtSelection('hello', 5, 5, ' world')).toEqual({
      next: 'hello world',
      caret: 11,
    });
  });

  it('replaces a non-empty selection', () => {
    expect(insertAtSelection('keep DROP keep', 5, 9, 'take')).toEqual({
      next: 'keep take keep',
      caret: 9,
    });
  });

  it('inserts into an empty draft', () => {
    expect(insertAtSelection('', 0, 0, 'body')).toEqual({
      next: 'body',
      caret: 4,
    });
  });

  it('keeps a multi-line body verbatim, blank edges included', () => {
    // The registered text opens AND closes with a newline, and carries a blank
    // line in the middle. All three survive: nothing is trimmed and nothing is
    // added, so the draft reads exactly what the settings editor stored.
    const text = '\nfirst\n\nsecond\n';
    const { next, caret } = insertAtSelection('a|b', 2, 2, text);

    expect(next).toBe('a|\nfirst\n\nsecond\nb');
    expect(next).toContain(text);
    expect(caret).toBe(2 + text.length);
  });

  it('lands the caret at selectionStart + text.length however the range was given', () => {
    const draft = 'one two three';
    const text = 'INSERTED';

    // A backwards selection (anchor after focus) describes the same range, so
    // the caret is measured from the LOW end either way.
    const forwards = insertAtSelection(draft, 4, 7, text);
    const backwards = insertAtSelection(draft, 7, 4, text);

    expect(forwards).toEqual(backwards);
    expect(forwards.caret).toBe(4 + text.length);
  });

  it('clamps offsets left over from a longer draft', () => {
    // A caret captured against a draft that has since shrunk must not slice
    // past the end and smear `undefined` into the result.
    expect(insertAtSelection('short', 99, 120, '!')).toEqual({
      next: 'short!',
      caret: 6,
    });
  });
});
