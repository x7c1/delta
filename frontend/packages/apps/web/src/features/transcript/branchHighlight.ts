/**
 * Marker highlight for a branch's passage, shown while hovering its sub-thread
 * chip: every occurrence of the chip's text is marked in the body so you can
 * see at a glance what the branch was about. Highlighting all matches (not just
 * the originally selected one) sidesteps any need to disambiguate which
 * occurrence was selected, and reads more naturally than a permanent mark on a
 * single spot.
 *
 * Assistant text is Markdown-rendered, so injecting a `<mark>` into the source
 * is not viable; instead the CSS Custom Highlight API paints over the
 * already-rendered DOM via text Ranges, agnostic to how the text was produced.
 * The matching `::highlight(branch-origin)` rule lives in index.css.
 */

const HIGHLIGHT_NAME = 'branch-origin';

/** Minimal shape of the Custom Highlight API, which the DOM lib may not type. */
interface HighlightRegistry {
  set(name: string, highlight: object): void;
  delete(name: string): void;
}
interface HighlightConstructor {
  new (...ranges: Range[]): object;
}

function registry(): HighlightRegistry | undefined {
  return (CSS as unknown as { highlights?: HighlightRegistry }).highlights;
}

/**
 * Build a DOM Range for every (non-overlapping) occurrence of `quote` within
 * `root`'s rendered text, each spanning text nodes when the match crosses
 * inline elements (e.g. Markdown emphasis) or block elements (e.g. across
 * paragraphs).
 *
 * Matching ignores whitespace on both sides. The quote (a branch's title) comes
 * from a browser selection, which injects newlines at block boundaries that the
 * running text nodes do not contain, and may differ in spacing — so a literal
 * search would miss any passage that crosses a paragraph, list item, or
 * heading. Stripping whitespace from both the needle and the haystack (while
 * mapping each kept character back to its node and offset) makes the match
 * robust to those differences; each resulting Range still spans the original
 * passage, internal whitespace included.
 */
export function findAllQuoteRanges(root: Node, quote: string): Range[] {
  const needle = quote.replace(/\s+/g, '');
  if (!needle) {
    return [];
  }

  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const positions: Array<{ node: Text; offset: number }> = [];
  let stripped = '';
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const textNode = node as Text;
    const data = textNode.data;
    for (let i = 0; i < data.length; i++) {
      if (!/\s/.test(data[i])) {
        stripped += data[i];
        positions.push({ node: textNode, offset: i });
      }
    }
  }

  const ranges: Range[] = [];
  for (
    let index = stripped.indexOf(needle);
    index >= 0;
    index = stripped.indexOf(needle, index + needle.length)
  ) {
    const start = positions[index];
    const end = positions[index + needle.length - 1];
    const range = document.createRange();
    range.setStart(start.node, start.offset);
    range.setEnd(end.node, end.offset + 1);
    ranges.push(range);
  }
  return ranges;
}

/**
 * Paint the branch-origin highlight over `ranges`, replacing any prior one.
 * An empty list clears the highlight.
 */
export function setBranchHighlight(ranges: Range[]): void {
  const highlights = registry();
  const HighlightCtor = (window as unknown as { Highlight?: HighlightConstructor })
    .Highlight;
  if (!highlights || !HighlightCtor) {
    return;
  }
  if (ranges.length === 0) {
    highlights.delete(HIGHLIGHT_NAME);
    return;
  }
  highlights.set(HIGHLIGHT_NAME, new HighlightCtor(...ranges));
}

/** Remove the branch-origin highlight, if any. */
export function clearBranchHighlight(): void {
  registry()?.delete(HIGHLIGHT_NAME);
}
