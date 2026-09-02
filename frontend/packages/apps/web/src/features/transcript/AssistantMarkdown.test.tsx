import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { AssistantMarkdown } from './AssistantMarkdown';

const PR_URL = 'https://github.com/x7c1/delta/pull/375';

describe('AssistantMarkdown', () => {
  it('keeps CJK punctuation out of an autolinked URL', () => {
    const { container } = render(
      <AssistantMarkdown text={`completed（PR: ${PR_URL}）。`} />,
    );
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', PR_URL);
    expect(link).toHaveTextContent(PR_URL);
    // The punctuation the autolinker absorbed is back in the prose.
    expect(container.textContent).toBe(`completed（PR: ${PR_URL}）。`);
  });

  it('keeps CJK punctuation out of a www autolink, scheme intact', () => {
    // GFM linked `www.…` under an `http://` scheme it added itself, and it
    // recognises that `www.` prefix case-insensitively.
    for (const prefix of ['www.', 'WWW.']) {
      const { container, getByRole, unmount } = render(
        <AssistantMarkdown text={`詳細は ${prefix}example.com/a）。次へ`} />,
      );
      const link = getByRole('link');
      expect(link).toHaveAttribute('href', `http://${prefix}example.com/a`);
      expect(link).toHaveTextContent(`${prefix}example.com/a`);
      expect(container.textContent).toBe(`詳細は ${prefix}example.com/a）。次へ`);
      unmount();
    }
  });

  it('trims every autolink in a paragraph, not just the first', () => {
    const text =
      '詳細は https://example.com/a）。続きは https://example.com/b」です';
    const { container } = render(<AssistantMarkdown text={text} />);
    const hrefs = screen
      .getAllByRole('link')
      .map((link) => link.getAttribute('href'));
    expect(hrefs).toEqual(['https://example.com/a', 'https://example.com/b']);
    expect(container.textContent).toBe(text);
  });

  it('leaves the URL an explicit link spells out alone', () => {
    render(<AssistantMarkdown text={`[PR](https://example.com/x）。)`} />);
    const link = screen.getByRole('link');
    // Percent-encoded on the way to HTML, but every character the author
    // wrote inside the parentheses survives — nothing was trimmed.
    expect(link).toHaveAttribute(
      'href',
      `https://example.com/x${encodeURIComponent('）。')}`,
    );
    expect(link).toHaveTextContent('PR');
  });

  it('leaves an explicit autolink alone', () => {
    const { container } = render(
      <AssistantMarkdown text={`<https://example.com/x>）。`} />,
    );
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', 'https://example.com/x');
    expect(link).toHaveTextContent('https://example.com/x');
    expect(container.textContent).toBe('https://example.com/x）。');
  });

  it('does not link a URL inside inline code', () => {
    const { container } = render(
      <AssistantMarkdown text={`run \`curl ${PR_URL}）。\` now`} />,
    );
    expect(screen.queryByRole('link')).toBeNull();
    expect(container.querySelector('code')).toHaveTextContent(
      `curl ${PR_URL}）。`,
    );
  });

  it('does not link a URL inside a fenced code block', () => {
    const { container } = render(
      <AssistantMarkdown text={'```\n' + `${PR_URL}）。\n` + '```\n'} />,
    );
    expect(screen.queryByRole('link')).toBeNull();
    expect(container.querySelector('pre code')).toHaveTextContent(
      `${PR_URL}）。`,
    );
  });
});
