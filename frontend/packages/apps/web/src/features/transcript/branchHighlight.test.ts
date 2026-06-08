import { describe, expect, it } from 'vitest';
import { findAllQuoteRanges } from './branchHighlight';

function root(html: string): HTMLElement {
  const el = document.createElement('div');
  el.innerHTML = html;
  return el;
}

describe('findAllQuoteRanges', () => {
  it('finds a quote within a single text node', () => {
    const ranges = findAllQuoteRanges(root('the quick brown fox'), 'quick brown');
    expect(ranges.map((r) => r.toString())).toEqual(['quick brown']);
  });

  it('finds a quote spanning inline elements (e.g. Markdown emphasis)', () => {
    // The visible text is "make it bold now"; the quote crosses the <strong>.
    const ranges = findAllQuoteRanges(
      root('make it <strong>bold</strong> now'),
      'it bold now',
    );
    expect(ranges.map((r) => r.toString())).toEqual(['it bold now']);
  });

  it('matches across block elements when the quote has a block-boundary newline', () => {
    // A browser selection across paragraphs yields "Heading\nbody text", but
    // the rendered text nodes carry no newline between the blocks. Matching
    // must ignore that whitespace so a cross-block selection still marks.
    const ranges = findAllQuoteRanges(
      root('<h2>Heading</h2><p>body text</p>'),
      'Heading\nbody text',
    );
    expect(ranges.map((r) => r.toString())).toEqual(['Headingbody text']);
  });

  it('ignores differences in internal whitespace', () => {
    const ranges = findAllQuoteRanges(root('quick brown fox'), 'quick   brown');
    expect(ranges.map((r) => r.toString())).toEqual(['quick brown']);
  });

  it('marks every occurrence of the quote', () => {
    const ranges = findAllQuoteRanges(
      root('<p>repeat me</p><p>and repeat me again</p>'),
      'repeat me',
    );
    expect(ranges).toHaveLength(2);
    expect(ranges.every((r) => r.toString() === 'repeat me')).toBe(true);
  });

  it('returns an empty list when the quote is not present', () => {
    expect(findAllQuoteRanges(root('hello world'), 'absent')).toEqual([]);
  });

  it('returns an empty list for an empty quote', () => {
    expect(findAllQuoteRanges(root('hello'), '')).toEqual([]);
  });
});
