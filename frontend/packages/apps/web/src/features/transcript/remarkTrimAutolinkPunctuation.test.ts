import { describe, expect, it } from 'vitest';
import {
  trimAutolinkPunctuation,
  trimGitHubIssueUrl,
} from './remarkTrimAutolinkPunctuation';

const PR_URL = 'https://github.com/x7c1/delta/pull/375';
const ISSUE_URL = 'https://github.com/x7c1/delta/issues/12';
const IRI = 'https://ja.wikipedia.org/wiki/デルタ（曖昧さ回避）';

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
    expect(trimAutolinkPunctuation(IRI)).toEqual({ url: IRI, suffix: '' });
  });

  it('strips only the closer the URL does not open itself', () => {
    expect(trimAutolinkPunctuation(`${IRI}）`)).toEqual({
      url: IRI,
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

  it('leaves non-punctuation text glued to a generic URL alone', () => {
    // Indistinguishable from an IRI path such as `…/wiki/東京`.
    const url = 'https://ja.wikipedia.org/wiki/東京です';
    expect(trimAutolinkPunctuation(url)).toEqual({ url, suffix: '' });
  });

  it('lets the GitHub rule decide a URL the generic rules would not cut', () => {
    // Only the GitHub rule cuts a balanced pair, so it has to run first.
    expect(trimAutolinkPunctuation(`${PR_URL}（補足）`)).toEqual({
      url: PR_URL,
      suffix: '（補足）',
    });
    // Where both rules apply they agree, and the GitHub one still answers.
    expect(trimAutolinkPunctuation(`${PR_URL}。（補足）`)).toEqual({
      url: PR_URL,
      suffix: '。（補足）',
    });
  });
});

describe('trimGitHubIssueUrl', () => {
  it('cuts at a full-width bracket the generic rules would call balanced', () => {
    expect(trimGitHubIssueUrl(`${PR_URL}（実機確認済み）`)).toEqual({
      url: PR_URL,
      suffix: '（実機確認済み）',
    });
    expect(trimGitHubIssueUrl(`${ISSUE_URL}「WIP」`)).toEqual({
      url: ISSUE_URL,
      suffix: '「WIP」',
    });
  });

  it('cuts at non-punctuation text glued to the number', () => {
    expect(trimGitHubIssueUrl(`${PR_URL}です`)).toEqual({
      url: PR_URL,
      suffix: 'です',
    });
  });

  it('keeps the ASCII tail GitHub itself puts after the number', () => {
    for (const tail of ['', '/', '/files', '#issuecomment-1', '?w=1']) {
      expect(trimGitHubIssueUrl(`${PR_URL}${tail}`)).toEqual({
        url: `${PR_URL}${tail}`,
        suffix: '',
      });
    }
  });

  it('recognises the shape through a www prefix and an uppercase host', () => {
    expect(trimGitHubIssueUrl('www.github.com/x7c1/delta/pull/375。')).toEqual({
      url: 'www.github.com/x7c1/delta/pull/375',
      suffix: '。',
    });
    expect(
      trimGitHubIssueUrl('HTTPS://GITHUB.COM/x7c1/delta/issues/12）'),
    ).toEqual({
      url: 'HTTPS://GITHUB.COM/x7c1/delta/issues/12',
      suffix: '）',
    });
  });

  it('does not recognise a GitHub path that is not a pull or an issue', () => {
    const tree = 'https://github.com/x7c1/delta/tree/main/デルタ';
    expect(trimGitHubIssueUrl(tree)).toBeUndefined();
    // …so the generic rules keep the path they cannot tell from an IRI.
    expect(trimAutolinkPunctuation(tree)).toEqual({ url: tree, suffix: '' });
  });

  it('does not recognise a non-GitHub URL', () => {
    expect(trimGitHubIssueUrl(IRI)).toBeUndefined();
  });
});
