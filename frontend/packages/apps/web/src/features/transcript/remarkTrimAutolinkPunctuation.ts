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
 * Known limit: non-punctuation text glued to a URL (`https://…/375です`) is
 * left alone. It is indistinguishable from an IRI path such as
 * `https://ja.wikipedia.org/wiki/東京`, where the same characters are part of
 * the address.
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
 * Splits an autolinked URL into the address itself and the CJK punctuation the
 * autolinker wrongly absorbed. `url + suffix` always reconstructs the input;
 * `suffix` is empty when there is nothing to trim.
 */
export function trimAutolinkPunctuation(url: string): {
  url: string;
  suffix: string;
} {
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
 * Remark plugin that moves absorbed CJK punctuation out of autolink literals
 * and back into the surrounding prose. Runs after `remark-gfm`, whose autolink
 * literals it post-processes.
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
