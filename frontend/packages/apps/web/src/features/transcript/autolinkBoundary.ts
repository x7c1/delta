import type { Root } from 'mdast';
import { visit } from 'unist-util-visit';

/**
 * Keeps CJK prose out of autolinked URLs.
 *
 * GFM's autolink-literal extension turns a bare `https://…` into a link by
 * reading up to the next whitespace and then dropping trailing *ASCII*
 * punctuation, and only when an end — whitespace, `<` or the end of input —
 * follows it. Full-width punctuation is neither an end nor trimmed, so a URL
 * written inside Japanese prose swallows whatever follows it — `…/pull/375）。`
 * becomes part of the href and the resulting link points nowhere. Worse, a
 * closing `**` right after the URL is absorbed along with the prose, so the
 * strong around it never closes. That is the behaviour the GFM spec mandates
 * (GitHub renders it the same way), so it is corrected here rather than waited
 * on upstream.
 *
 * The correction happens before parsing, because emphasis is resolved while
 * tokenizing and a delimiter the autolink swallowed cannot be given back
 * afterwards. `markAutolinkBoundaries` finds where each URL should end and
 * puts an invisible sentinel there, which GFM reads as whitespace: the link
 * ends at it, the ASCII punctuation just before it (`**`, `_`, `~~`, `.`) is
 * trimmed from the link as usual, and a closing emphasis delimiter there
 * closes. `remarkStripAutolinkBoundaries` then removes every sentinel from the
 * parsed tree so none reaches the DOM.
 *
 * Known limit: non-punctuation text glued to a URL
 * (`https://ja.wikipedia.org/wiki/東京です`) is left alone, and so is a
 * *balanced* full-width pair. Both are indistinguishable from an IRI path such
 * as `https://ja.wikipedia.org/wiki/デルタ（曖昧さ回避）`, where the same
 * characters are part of the address. GitHub pull-request and issue URLs are
 * the exception `trimGitHubIssueUrl` recovers.
 */

/**
 * Characters that terminate a sentence or separate clauses. The URL ends before
 * the first one: in practice they come from the prose around it. An IRI that
 * does carry one unencoded — a Wikipedia title ending in `！`, say — loses its
 * tail, which is the rarer and the less confusing of the two mistakes.
 */
const TERMINATORS = new Set('。、，．：；！？');

/**
 * Full-width bracket and quote pairs, keyed by the closing character. A closer
 * ends the URL only when it has no matching opener earlier in it, so a URL that
 * genuinely contains a pair — `https://ja.wikipedia.org/wiki/デルタ（曖昧さ回避）`
 * — keeps it.
 */
const OPENER_BY_CLOSER = new Map([
  ['）', '（'],
  ['」', '「'],
  ['』', '『'],
  ['】', '【'],
  ['〉', '〈'],
  ['》', '《'],
  ['〕', '〔'],
  ['｝', '｛'],
  ['］', '［'],
]);

const OPENERS = new Set(OPENER_BY_CLOSER.values());

/**
 * A GitHub pull-request or issue address, up to and including its number:
 * an optional scheme, an optional `www.`, the host, owner and repo, then
 * `pull/` or `issues/` and the number. Matched case-insensitively, as GFM
 * autolinks `WWW.GitHub.com/…` just as readily as the lowercase spelling.
 */
const GITHUB_ISSUE_PREFIX =
  /^(?:https?:\/\/)?(?:www\.)?github\.com\/[^/]+\/[^/]+\/(?:pull|issues)\/\d+/i;

/**
 * The highest code point GitHub can put after an issue number: everything a
 * pull-request or issue URL carries past it — `/files`, `#issuecomment-1234`,
 * `?w=1` — is ASCII.
 */
const LAST_ASCII_CODE_POINT = 0x7f;

/**
 * Splits a GitHub pull-request or issue URL at the first non-ASCII character
 * after the issue number, or returns `undefined` when the URL is not that
 * shape. Unlike the generic rules this needs no notion of balance: the path
 * past the number is ASCII by construction, so any non-ASCII character there
 * came from the prose — `…/pull/375（実機確認済み）` and `…/pull/375です` alike.
 */
export function trimGitHubIssueUrl(
  url: string,
): { url: string; suffix: string } | undefined {
  const match = GITHUB_ISSUE_PREFIX.exec(url);
  if (match === null) {
    return undefined;
  }
  let offset = match[0].length;
  while (
    offset < url.length &&
    url.charCodeAt(offset) <= LAST_ASCII_CODE_POINT
  ) {
    offset += 1;
  }
  return { url: url.slice(0, offset), suffix: url.slice(offset) };
}

/**
 * Splits an autolink-literal candidate — the text GFM's autolinker would read,
 * up to the next whitespace — into the address itself and the trailing text
 * that belongs to the prose. `url + suffix` always reconstructs the input;
 * `suffix` is empty when there is nothing to trim.
 *
 * `trimGitHubIssueUrl` is tried first and, where it applies, answers on its
 * own: only it cuts a balanced pair such as `…/pull/375（補足）`.
 */
export function trimAutolinkPunctuation(url: string): {
  url: string;
  suffix: string;
} {
  const gitHub = trimGitHubIssueUrl(url);
  if (gitHub !== undefined) {
    return gitHub;
  }
  const openCounts = new Map<string, number>();
  const characters = [...url];
  let offset = 0;

  for (const character of characters) {
    if (TERMINATORS.has(character)) {
      break;
    }
    const opener = OPENER_BY_CLOSER.get(character);
    if (opener !== undefined) {
      const open = openCounts.get(opener) ?? 0;
      if (open === 0) {
        // A closer with no opener before it belongs to the surrounding prose.
        break;
      }
      openCounts.set(opener, open - 1);
    } else if (OPENERS.has(character)) {
      openCounts.set(character, (openCounts.get(character) ?? 0) + 1);
    }
    offset += character.length;
  }

  return { url: url.slice(0, offset), suffix: url.slice(offset) };
}

/**
 * The boundary `markAutolinkBoundaries` inserts: U+FEFF, zero-width and, to
 * micromark, Unicode whitespace — `micromark-util-character` classifies it with
 * JavaScript's `\s`, which includes U+FEFF. Relying on that is relying on the
 * parser's implementation; `AssistantMarkdown.test.tsx` pins it, so a
 * dependency update that changes it fails the tests.
 */
const BOUNDARY = '\uFEFF';

/**
 * An autolink-literal candidate: a `http://`, `https://` or `www.` start,
 * case-insensitively, and everything up to the next whitespace or `<` — the
 * span GFM's autolinker reads. It is looser than GFM's own start conditions
 * (no check of the preceding character or of the domain), which is harmless:
 * a boundary placed in text GFM does not link is stripped again after parsing.
 */
const AUTOLINK_CANDIDATE = /(?:https?:\/\/|www\.)[^\s<]*/gi;

/**
 * Inserts a boundary into `markdown` wherever `trimAutolinkPunctuation` would
 * end an autolink-literal candidate, so GFM ends the link there itself. The
 * text is otherwise unchanged; `remarkStripAutolinkBoundaries` removes the
 * boundaries from the parsed tree.
 */
export function markAutolinkBoundaries(markdown: string): string {
  return markdown.replace(AUTOLINK_CANDIDATE, (candidate) => {
    const { url, suffix } = trimAutolinkPunctuation(candidate);
    return suffix === '' ? candidate : `${url}${BOUNDARY}${suffix}`;
  });
}

/**
 * Remark plugin that removes every boundary `markAutolinkBoundaries` put into
 * the source, from every string field of every node. That covers more than
 * text `value`s: a boundary lands wherever the cut fell, including an explicit
 * link's destination (`[wiki](https://ja.wikipedia.org/wiki/東京。)`), a
 * reference label, a code block's info string or raw HTML. Left in place it
 * would reach the DOM, invisible but carried along by a copy. A U+FEFF the
 * text already carried is indistinguishable from a boundary and is removed too.
 */
export function remarkStripAutolinkBoundaries() {
  return (tree: Root) => {
    visit(tree, (node) => {
      const fields = node as unknown as Record<string, unknown>;
      for (const [key, value] of Object.entries(fields)) {
        if (typeof value === 'string' && value.includes(BOUNDARY)) {
          fields[key] = value.replaceAll(BOUNDARY, '');
        }
      }
    });
  };
}
