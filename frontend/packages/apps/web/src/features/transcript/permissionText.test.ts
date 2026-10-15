import { describe, expect, it } from 'vitest';
import {
  ELIDE_LIMIT,
  escapeInvisible,
  headingText,
  toolArguments,
  visibleText,
  type TextSegment,
} from './permissionText';

/** The segments joined back into the text the card shows. */
const shown = (segments: TextSegment[]) =>
  segments.map((segment) => segment.text).join('');

describe('visibleText', () => {
  it('leaves ordinary text, indentation and short runs as they are', () => {
    const text = 'fn main() {\n        println!("hi");\n}\n';
    const visible = visibleText(text);

    expect(shown(visible.segments)).toBe(text);
    expect(visible.elided).toBe(0);
  });

  it('collapses a long run of blank lines inside a value', () => {
    const visible = visibleText('top\n\n\n\n\n\nbottom');

    expect(shown(visible.segments)).toBe('top[6 line breaks]bottom');
  });

  it('keeps a byte-order mark visible inside leading padding', () => {
    // The mark must not be counted as one more space of the collapsed run.
    const visible = visibleText(`\uFEFF${' '.repeat(10)}ls`);

    expect(shown(visible.segments)).toBe('\\u{FEFF}[10 spaces]ls');
  });

  it('never splits a surrogate pair when eliding', () => {
    const text = '\u{1F600}'.repeat(ELIDE_LIMIT + 1);
    const visible = visibleText(text);

    expect(shown(visible.segments)).not.toMatch(
      /[\uD800-\uDFFF](?![\uDC00-\uDFFF])/u,
    );
    expect(visible.elided).toBeGreaterThan(0);
  });
});

describe('escapeInvisible', () => {
  it('escapes bidi controls, isolates and zero-width characters', () => {
    expect(escapeInvisible('a\u202Ab\u2066c\u200Dd\u2060e')).toBe(
      'a\\u{202A}b\\u{2066}c\\u{200D}d\\u{2060}e',
    );
  });

  it('escapes C1 controls and carriage returns, keeping tab and newline', () => {
    expect(escapeInvisible('a\r\n\tb\u0085')).toBe('a\\u{D}\n\tb\\u{85}');
  });
});

describe('headingText', () => {
  it('keeps the head and the tail of a long heading', () => {
    const heading = headingText(`${'a'.repeat(100)}END`, 80);

    expect(heading.startsWith('a')).toBe(true);
    expect(heading.endsWith('END')).toBe(true);
    expect(heading).toContain(' … ');
  });
});

describe('toolArguments', () => {
  it('puts what runs and where first, then the rest in input order', () => {
    const args = toolArguments(
      JSON.stringify({ itemId: 'i', zeta: 1, cwd: '/w', command: 'ls' }),
    );

    expect(args.map((arg) => arg.label)).toEqual([
      'command',
      'cwd',
      'itemId',
      'zeta',
    ]);
  });

  it('shows a non-string value as indented JSON', () => {
    const [arg] = toolArguments(JSON.stringify({ edits: [{ a: 1 }] }));

    expect(arg.value).toBe('[\n  {\n    "a": 1\n  }\n]');
  });
});
