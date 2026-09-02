import { describe, expect, it } from 'vitest';
import { trimAutolinkPunctuation } from './remarkTrimAutolinkPunctuation';

const PR_URL = 'https://github.com/x7c1/delta/pull/375';

describe('trimAutolinkPunctuation', () => {
  it('leaves a clean URL untouched, with an empty suffix', () => {
    expect(trimAutolinkPunctuation(PR_URL)).toEqual({
      url: PR_URL,
      suffix: '',
    });
  });

  it('splits off a closing paren followed by a full stop', () => {
    expect(trimAutolinkPunctuation(`${PR_URL}）。`)).toEqual({
      url: PR_URL,
      suffix: '）。',
    });
  });

  it('cuts at each of the terminating punctuation marks', () => {
    for (const terminator of [...'。、，．：；！？']) {
      expect(trimAutolinkPunctuation(`${PR_URL}${terminator}続き`)).toEqual({
        url: PR_URL,
        suffix: `${terminator}続き`,
      });
    }
  });

  it('cuts at an unbalanced closing quote and the text after it', () => {
    expect(trimAutolinkPunctuation('https://example.com/a?b=1」です')).toEqual({
      url: 'https://example.com/a?b=1',
      suffix: '」です',
    });
  });

  it('strips every unbalanced closer in the tail', () => {
    for (const closer of [...'）」』】〉》〕｝］']) {
      expect(trimAutolinkPunctuation(`${PR_URL}${closer}`)).toEqual({
        url: PR_URL,
        suffix: closer,
      });
    }
  });

  it('keeps a balanced full-width pair inside the URL', () => {
    const iri = 'https://ja.wikipedia.org/wiki/デルタ（曖昧さ回避）';
    expect(trimAutolinkPunctuation(iri)).toEqual({ url: iri, suffix: '' });
  });

  it('strips only the closer the URL does not open itself', () => {
    const iri = 'https://ja.wikipedia.org/wiki/デルタ（曖昧さ回避）';
    expect(trimAutolinkPunctuation(`${iri}）`)).toEqual({
      url: iri,
      suffix: '）',
    });
  });

  it('leaves ASCII punctuation alone — GFM has already trimmed it', () => {
    // An ASCII paren that survived GFM's own trimming is the author's.
    expect(trimAutolinkPunctuation(`${PR_URL}(a)`)).toEqual({
      url: `${PR_URL}(a)`,
      suffix: '',
    });
  });

  it('leaves non-punctuation text glued to a URL alone', () => {
    // Indistinguishable from an IRI path such as `…/wiki/東京`.
    expect(trimAutolinkPunctuation(`${PR_URL}です`)).toEqual({
      url: `${PR_URL}です`,
      suffix: '',
    });
  });
});
