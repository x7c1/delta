import { describe, expect, it, vi } from 'vitest';
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

  // Relative and `#fragment` links are deliberately not exempt.
  it.each([
    ['an autolinked URL', `see ${PR_URL} for details`, PR_URL, PR_URL],
    ['an explicit link', `[the PR](${PR_URL})`, PR_URL, 'the PR'],
    ['a relative link', '[doc](/docs/a)', '/docs/a', 'doc'],
    ['a fragment link', '[doc](#section)', '#section', 'doc'],
  ])(
    'opens %s in a new tab, href and label intact',
    (_shape, text, href, label) => {
      render(<AssistantMarkdown text={text} />);
      const link = screen.getByRole('link');
      expect(link).toHaveAttribute('href', href);
      expect(link).toHaveTextContent(label);
      expect(link).toHaveAttribute('target', '_blank');
      // `noopener` is what keeps the opened page from reaching back through
      // `window.opener`; it travels with `target` or not at all.
      expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    },
  );

  it('keeps the title a Markdown link spells out', () => {
    render(<AssistantMarkdown text={`[the PR](${PR_URL} "hover me")`} />);
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('title', 'hover me');
    expect(link).toHaveAttribute('target', '_blank');
  });

  it('keeps the attributes GFM generates for a footnote round trip', () => {
    const { container } = render(
      <AssistantMarkdown text={'A claim[^1]\n\n[^1]: The evidence.\n'} />,
    );
    const [reference, backReference] = screen.getAllByRole('link');
    // The footnote's `↩` link points back at the reference by id, so dropping
    // that id would leave the reader's way back to the prose aimed at nothing.
    expect(reference).toHaveAttribute('href', '#user-content-fn-1');
    expect(backReference).toHaveAttribute('href', '#user-content-fnref-1');
    expect(container.querySelector('#user-content-fnref-1')).toBe(reference);
    // What a screen reader announces at each end of that jump.
    expect(reference).toHaveAttribute('aria-describedby', 'footnote-label');
    expect(backReference).toHaveAttribute('aria-label', 'Back to reference 1');
    // Forcing `target`/`rel` onto these anchors costs them none of the above.
    expect(reference).toHaveAttribute('target', '_blank');
    expect(backReference).toHaveAttribute('target', '_blank');
  });

  it('does not forward react-markdown internals to the anchor element', () => {
    // React reports an unknown DOM attribute through `console.error`, which is
    // what spreading the mdast `node` onto the `<a>` would produce.
    const consoleError = vi
      .spyOn(console, 'error')
      .mockImplementation(() => {});
    try {
      render(<AssistantMarkdown text={`[the PR](${PR_URL})`} />);
      expect(screen.getByRole('link')).not.toHaveAttribute('node');
      expect(consoleError).not.toHaveBeenCalled();
    } finally {
      consoleError.mockRestore();
    }
  });
});
