import type { Link, Parent, Root, Text } from 'mdast';
import { SKIP, visit } from 'unist-util-visit';

/**
 * Keeps CJK punctuation out of autolinked URLs.
 *
 * GFM's autolink-literal extension turns a bare `https://…` into a link by
 * reading up to the next whitespace and then trimming only *ASCII* trailing
 * punctuation. Full-width punctuation is not trimmed, so a URL written inside
 * Japanese prose swallows whatever follows it — `…/pull/375）。` becomes part
 * of the href and the resulting link points nowhere. That is the behaviour the
 * GFM spec mandates (GitHub renders it the same way), so it is corrected here,
 * after parsing, rather than waited on upstream.
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
 * Splits an autolinked URL into the address itself and the trailing text the
 * autolinker wrongly absorbed. `url + suffix` always reconstructs the input;
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
 * The scheme GFM prepends to a `www.…` autolink literal, whose url therefore is
 * its text with `http://` in front rather than the text itself.
 */
const WWW_URL_PREFIX = 'http://';

/**
 * The text child of an autolink literal, or `undefined` for any other link.
 * GFM's autolink literals are a link whose only child is the address as
 * written: identical to `node.url` for `https://…`, and `node.url` minus the
 * `http://` it prepends for `www.…`. An explicit `[label](url)` carries a
 * different label, and its URL was chosen by the author, so it is left
 * untouched.
 */
function autolinkLiteralText(node: Link): Text | undefined {
  const [child] = node.children;
  if (node.children.length !== 1 || child === undefined) {
    return undefined;
  }
  if (child.type !== 'text') {
    return undefined;
  }
  if (child.value === node.url) {
    return child;
  }
  // GFM recognises the prefix case-insensitively, so `WWW.…` counts too.
  const isWww =
    child.value.toLowerCase().startsWith('www.') &&
    node.url === `${WWW_URL_PREFIX}${child.value}`;
  return isWww ? child : undefined;
}

/**
 * Remark plugin that moves the text an autolink literal wrongly absorbed back
 * into the surrounding prose. Runs after `remark-gfm`, whose autolink literals
 * it post-processes.
 */
export function remarkTrimAutolinkPunctuation() {
  return (tree: Root) => {
    visit(tree, 'link', (node: Link, index, parent: Parent | undefined) => {
      if (parent === undefined || index === undefined) {
        return;
      }
      const child = autolinkLiteralText(node);
      if (child === undefined) {
        return;
      }
      const { url, suffix } = trimAutolinkPunctuation(child.value);
      if (suffix === '') {
        return;
      }
      // Keep whatever scheme GFM put in front of the text it linked.
      const scheme = node.url.slice(0, node.url.length - child.value.length);
      node.url = `${scheme}${url}`;
      child.value = url;
      const trailing: Text = { type: 'text', value: suffix };
      parent.children.splice(index + 1, 0, trailing);
      // Continue after the text node just inserted.
      return [SKIP, index + 2];
    });
  };
}
