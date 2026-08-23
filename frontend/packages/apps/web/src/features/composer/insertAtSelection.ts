/**
 * The outcome of splicing text into a draft: the whole new draft plus where the
 * caret belongs afterwards.
 */
export interface DraftInsertion {
  /** The draft after the insertion. */
  next: string;
  /** Caret offset within {@link next}, immediately after the inserted text. */
  caret: number;
}

/** Keep an offset inside `[0, length]`. */
function clamp(value: number, length: number): number {
  return Math.min(Math.max(value, 0), length);
}

/**
 * Replace `[selectionStart, selectionEnd)` of `draft` with `text` and report
 * where the caret lands — the pure core of "insert a prompt template at the
 * cursor", extracted from the component so it can be reasoned about (and
 * tested) without a DOM.
 *
 * `text` is spliced in VERBATIM: no separator, no newline, no trimming, at
 * either end. A template's body is written by the user in the settings editor,
 * so its leading and trailing whitespace is content — a template that
 * deliberately opens with a blank line must still open with one after being
 * inserted. Anything the caller wants around it belongs in the template.
 *
 * The offsets are normalized rather than trusted: they are ordered (a backwards
 * selection is the same range as a forwards one) and clamped to the draft, so a
 * stale caret left over from a longer draft cannot produce `undefined`-laced
 * output. With an empty range this is a plain insertion; with a non-empty one
 * the selected text is replaced.
 */
export function insertAtSelection(
  draft: string,
  selectionStart: number,
  selectionEnd: number,
  text: string,
): DraftInsertion {
  const start = clamp(Math.min(selectionStart, selectionEnd), draft.length);
  const end = clamp(Math.max(selectionStart, selectionEnd), draft.length);
  return {
    next: `${draft.slice(0, start)}${text}${draft.slice(end)}`,
    caret: start + text.length,
  };
}
