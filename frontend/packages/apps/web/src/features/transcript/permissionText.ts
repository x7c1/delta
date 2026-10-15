/**
 * Turning the text of a permission request into what the card renders.
 *
 * The card is where the user decides whether a tool may run, so it must show
 * the request as it is: every character that will reach the tool, in order,
 * with nothing hidden by layout. Three things can hide text even when it is
 * all on the page, and each is made visible here rather than left to CSS:
 *
 * - **Invisible and direction-changing characters.** A bidi override can make
 *   the tail of a command read as something else, and a zero-width or control
 *   character can sit between two that look adjacent. Each is shown as a
 *   visible escape (`\u{202E}`), and the card warns that the request has them.
 * - **Long runs of whitespace or blank lines.** Padding can push the part of a
 *   value that matters below or beside what the eye takes in. A run longer
 *   than a few characters at either end of a value, or a much longer one in
 *   its middle, is collapsed to a marker that states its length.
 * - **Length.** A value longer than {@link ELIDE_LIMIT} shows its head and its
 *   tail with the elided count between them — never only the head, since the
 *   end of a command is as decisive as its start.
 */

/** Values longer than this many characters are shown as head + tail. */
export const ELIDE_LIMIT = 4000;
/** How many leading characters an elided value keeps. */
export const ELIDE_HEAD = 2400;
/** How many trailing characters an elided value keeps. */
export const ELIDE_TAIL = 1200;

/**
 * A run of whitespace at the start or end of a value longer than this many
 * characters becomes a marker. Short, so padding cannot hide at the edges.
 */
const EDGE_RUN_LIMIT = 3;
/**
 * A run of spaces inside a value longer than this many characters becomes a
 * marker. Longer than the edge limit so ordinary indentation in code is left
 * as it is.
 */
const INNER_RUN_LIMIT = 32;
/** More consecutive blank lines than this inside a value become a marker. */
const INNER_BLANK_LINES_LIMIT = 3;

/** One piece of a rendered value. */
export type TextSegment =
  /** Text shown as it is. */
  | { kind: 'text'; text: string }
  /** An invisible or direction-changing character, shown as its escape. */
  | { kind: 'escape'; text: string }
  /** A collapsed whitespace run or elided middle, described in words. */
  | { kind: 'marker'; text: string };

/**
 * Characters that render as nothing or change the direction or flow of what
 * follows: C0 and C1 controls other than tab and newline, the soft hyphen, the
 * Arabic letter mark, zero-width characters, directional marks, the line and
 * paragraph separators, embeddings, overrides and isolates, the word joiner
 * and invisible operators, and the byte-order mark.
 */
const INVISIBLE_CLASS =
  '[\\u0000-\\u0008\\u000b-\\u001f\\u007f-\\u009f\\u00ad\\u061c\\u180e' +
  '\\u200b-\\u200f\\u2028-\\u202e\\u2060-\\u2064\\u2066-\\u2069\\ufeff]';
const INVISIBLE = new RegExp(INVISIBLE_CLASS, 'u');
const INVISIBLE_GLOBAL = new RegExp(INVISIBLE_CLASS, 'gu');

/**
 * Horizontal whitespace: spaces, tabs and the Unicode space separators. Kept
 * disjoint from {@link INVISIBLE_CLASS}, so a collapsed run never swallows a
 * character that must be shown as an escape.
 */
const SPACE_CLASS = '[ \\t\\u00a0\\u1680\\u2000-\\u200a\\u202f\\u205f\\u3000]';
/** Any whitespace a run may hold: horizontal whitespace or a newline. */
const WHITESPACE_CLASS =
  '[\\n \\t\\u00a0\\u1680\\u2000-\\u200a\\u202f\\u205f\\u3000]';
/** A blank line's worth of text: optional horizontal whitespace, then `\n`. */
const BLANK_LINE = `(?:${SPACE_CLASS}*\\n)`;

/** Whether `text` contains a character {@link escapeInvisible} would escape. */
export function hasInvisible(text: string): boolean {
  return INVISIBLE.test(text);
}

/** The visible escape for one code point, e.g. `\u{202E}`. */
function escapeOf(char: string): string {
  const code = char.codePointAt(0) ?? 0;
  return `\\u{${code.toString(16).toUpperCase()}}`;
}

/** `text` with each invisible character replaced by its escape, as one string. */
export function escapeInvisible(text: string): string {
  return text.replace(INVISIBLE_GLOBAL, escapeOf);
}

/** Split plain text into text and escape segments. */
function escapeSegments(text: string): TextSegment[] {
  const segments: TextSegment[] = [];
  let last = 0;
  for (const match of text.matchAll(INVISIBLE_GLOBAL)) {
    const index = match.index;
    if (index > last) {
      segments.push({ kind: 'text', text: text.slice(last, index) });
    }
    segments.push({ kind: 'escape', text: escapeOf(match[0]) });
    last = index + match[0].length;
  }
  if (last < text.length) {
    segments.push({ kind: 'text', text: text.slice(last) });
  }
  return segments;
}

/** Words for a collapsed run: how many spaces, tabs and line breaks it held. */
function describeRun(run: string): string {
  const newlines = (run.match(/\n/g) ?? []).length;
  const tabs = (run.match(/\t/g) ?? []).length;
  const spaces = Array.from(run).length - newlines - tabs;
  const parts: string[] = [];
  if (newlines > 0) {
    parts.push(`${newlines} line ${newlines === 1 ? 'break' : 'breaks'}`);
  }
  if (spaces > 0) {
    parts.push(`${spaces} ${spaces === 1 ? 'space' : 'spaces'}`);
  }
  if (tabs > 0) {
    parts.push(`${tabs} ${tabs === 1 ? 'tab' : 'tabs'}`);
  }
  return `[${parts.join(', ')}]`;
}

/**
 * The ranges of `text` that are whitespace runs long enough to collapse: one
 * at the start and one at the end (any whitespace, over
 * {@link EDGE_RUN_LIMIT}), and inside the value long runs of spaces (over
 * {@link INNER_RUN_LIMIT}) or of blank lines (over
 * {@link INNER_BLANK_LINES_LIMIT}).
 */
function paddingRanges(text: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  // An edge run still counts as one when invisible characters stand between
  // it and the edge; those characters stay outside the run, so they are
  // escaped rather than folded into the marker.
  const leading = new RegExp(
    `^${INVISIBLE_CLASS}*(${WHITESPACE_CLASS}+)`,
    'u',
  ).exec(text);
  if (leading) {
    const run = leading[1];
    const start = leading[0].length - run.length;
    ranges.push([start, start + run.length]);
  }
  const trailing = new RegExp(
    `(${WHITESPACE_CLASS}+)${INVISIBLE_CLASS}*$`,
    'u',
  ).exec(text);
  if (trailing && trailing.index >= (ranges[0]?.[1] ?? 0)) {
    ranges.push([trailing.index, trailing.index + trailing[1].length]);
  }
  for (let i = ranges.length - 1; i >= 0; i -= 1) {
    const [start, end] = ranges[i];
    if (Array.from(text.slice(start, end)).length <= EDGE_RUN_LIMIT) {
      ranges.splice(i, 1);
    }
  }
  const inner = new RegExp(
    `${SPACE_CLASS}{${INNER_RUN_LIMIT + 1},}|\\n${BLANK_LINE}{${INNER_BLANK_LINES_LIMIT + 1},}`,
    'gu',
  );
  for (const match of text.matchAll(inner)) {
    const start = match.index;
    const end = start + match[0].length;
    if (!ranges.some(([from, to]) => start < to && end > from)) {
      ranges.push([start, end]);
    }
  }
  return ranges.sort((a, b) => a[0] - b[0]);
}

/** Segment `text` with padding collapsed and invisible characters escaped. */
function visibleSegments(text: string): TextSegment[] {
  const segments: TextSegment[] = [];
  let last = 0;
  for (const [start, end] of paddingRanges(text)) {
    segments.push(...escapeSegments(text.slice(last, start)));
    segments.push({
      kind: 'marker',
      text: describeRun(text.slice(start, end)),
    });
    last = end;
  }
  segments.push(...escapeSegments(text.slice(last)));
  return segments;
}

/** A value prepared for the card. */
export interface VisibleText {
  /** The pieces to render, in order. */
  segments: TextSegment[];
  /** How many characters the middle elision left out (0 when none). */
  elided: number;
}

/**
 * Prepare `text` for the card: elide the middle of a value longer than
 * {@link ELIDE_LIMIT} (unless `full`), collapse padding, and escape invisible
 * characters. Lengths count code points, so a cut never splits a surrogate
 * pair.
 */
export function visibleText(text: string, full = false): VisibleText {
  const chars = Array.from(text);
  if (full || chars.length <= ELIDE_LIMIT) {
    return { segments: visibleSegments(text), elided: 0 };
  }
  const elided = chars.length - ELIDE_HEAD - ELIDE_TAIL;
  return {
    segments: [
      ...visibleSegments(chars.slice(0, ELIDE_HEAD).join('')),
      { kind: 'marker', text: `[… ${elided} characters elided …]` },
      ...visibleSegments(chars.slice(chars.length - ELIDE_TAIL).join('')),
    ],
    elided,
  };
}

/**
 * A one-line form of `text` for a heading: invisible characters escaped and,
 * past `limit` characters, the head and the tail around an ellipsis.
 */
export function headingText(text: string, limit = 80): string {
  const escaped = escapeInvisible(text);
  const chars = Array.from(escaped);
  if (chars.length <= limit) {
    return escaped;
  }
  const tail = Math.floor(limit / 3);
  const head = limit - tail;
  return `${chars.slice(0, head).join('')} … ${chars.slice(chars.length - tail).join('')}`;
}

/** One labelled argument of a tool input. */
export interface ToolArgument {
  /** The field name, or `null` for an input that is not an object. */
  label: string | null;
  /** The value as text: a string as it is, anything else as indented JSON. */
  value: string;
}

/**
 * Fields shown first, in this order, when the input has them: what a call
 * runs and where. Every other field follows in the input's own order.
 */
const LEADING_FIELDS = [
  'command',
  'cwd',
  'file_path',
  'notebook_path',
  'path',
  'url',
];

/**
 * Every argument of a tool input JSON, labelled. An object yields one entry
 * per field; anything else (an array, a scalar, or text that is not JSON)
 * yields a single unlabelled entry with the input as it is.
 */
export function toolArguments(toolInputJson: string): ToolArgument[] {
  let input: unknown;
  try {
    input = JSON.parse(toolInputJson);
  } catch {
    return [{ label: null, value: toolInputJson }];
  }
  if (typeof input !== 'object' || input === null || Array.isArray(input)) {
    return [{ label: null, value: toolInputJson }];
  }
  const record = input as Record<string, unknown>;
  const keys = Object.keys(record);
  const ordered = [
    ...LEADING_FIELDS.filter((key) => keys.includes(key)),
    ...keys.filter((key) => !LEADING_FIELDS.includes(key)),
  ];
  return ordered.map((key) => ({
    label: key,
    value: argumentText(record[key]),
  }));
}

/** A field's value as text: a string as it is, anything else as JSON. */
function argumentText(value: unknown): string {
  if (typeof value === 'string') {
    return value;
  }
  return JSON.stringify(value, null, 2) ?? String(value);
}
