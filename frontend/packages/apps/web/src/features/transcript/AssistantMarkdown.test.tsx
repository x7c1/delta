import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { AssistantMarkdown } from './AssistantMarkdown';

const PR_URL = 'https://github.com/x7c1/delta/pull/375';

/**
 * Asserts that no text node and no attribute under `container` carries the
 * U+FEFF boundary the renderer puts into the source before parsing.
 */
function expectNoBoundary(container: HTMLElement) {
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    expect(node.nodeValue).not.toContain('\uFEFF');
  }
  for (const element of container.querySelectorAll('*')) {
    for (const attribute of element.attributes) {
      expect(attribute.value).not.toContain('\uFEFF');
    }
  }
}

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

  // The generic rules clear neither tail: they read the parentheses as balanced
  // and leave them in the href, and `です` is not punctuation at all. So the
  // GitHub rule is what rescues the href here, on the text GFM really did read
  // up to the space.
  it.each([
    ['a balanced bracket', '（実機確認済み）。'],
    ['prose glued to the number', 'です'],
  ])('ends a pull-request URL at %s', (_shape, tail) => {
    const { container } = render(
      <AssistantMarkdown text={`詳細は ${PR_URL}${tail}`} />,
    );
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', PR_URL);
    // Exact, not `toHaveTextContent`'s substring match, which would pass even
    // with the tail painted inside the link.
    expect(link.textContent).toBe(PR_URL);
    // What the link absorbed is back in the prose, right after it.
    expect(link.nextSibling?.textContent).toBe(tail);
    expect(container.textContent).toBe(`詳細は ${PR_URL}${tail}`);
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

  describe('an autolink wrapped in emphasis and followed by prose', () => {
    const BOLD_PR_SENTENCE =
      'PR を作りました: **https://github.com/x7c1/delta/pull/229**（検査は通過済み、CI は実行中です）';

    it('closes the strong and keeps the delimiters out of the href', () => {
      const { container } = render(
        <AssistantMarkdown text={BOLD_PR_SENTENCE} />,
      );
      const strong = container.querySelector('strong');
      const link = within(strong!).getByRole('link');
      expect(link).toHaveAttribute(
        'href',
        'https://github.com/x7c1/delta/pull/229',
      );
      expect(link.textContent).toBe('https://github.com/x7c1/delta/pull/229');
      expect(strong!.nextSibling?.textContent).toBe(
        '（検査は通過済み、CI は実行中です）',
      );
      expect(container.textContent).not.toContain('**');
      expectNoBoundary(container);
    });

    it.each([
      [
        'a generic URL in bold',
        '**https://example.com/a**。',
        'strong',
        'https://example.com/a',
        '。',
      ],
      [
        'a pull-request URL in emphasis',
        `_${PR_URL}_（補足）`,
        'em',
        PR_URL,
        '（補足）',
      ],
      [
        'a pull-request URL in strikethrough',
        `~~${PR_URL}~~（補足）`,
        'del',
        PR_URL,
        '（補足）',
      ],
      [
        'a www pull-request URL in bold',
        '**www.github.com/x7c1/delta/pull/229**（補足）',
        'strong',
        'http://www.github.com/x7c1/delta/pull/229',
        '（補足）',
      ],
      [
        'a generic URL in bold with a bracket whose closer is past a space',
        '**http://localhost:5175/**（5174 は別の Vite が使っていました）',
        'strong',
        'http://localhost:5175/',
        '（5174 は別の Vite が使っていました）',
      ],
    ])('closes around %s', (_shape, text, tag, href, tail) => {
      const { container } = render(<AssistantMarkdown text={text} />);
      const wrapper = container.querySelector(tag);
      expect(wrapper).not.toBeNull();
      const link = within(wrapper as HTMLElement).getByRole('link');
      expect(link).toHaveAttribute('href', href);
      expect(wrapper!.nextSibling?.textContent).toBe(tail);
      expect(container.textContent).not.toMatch(/[*_~]/);
      expectNoBoundary(container);
    });
  });

  it('keeps an explicit destination that contains a cut point whole', () => {
    const destination = 'https://ja.wikipedia.org/wiki/東京。';
    const { container } = render(
      <AssistantMarkdown text={`[wiki](${destination})`} />,
    );
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', encodeURI(destination));
    expect(link.getAttribute('href')).not.toContain('%EF%BB%BF');
    expectNoBoundary(container);
  });

  it('renders code containing a cut point as written', () => {
    const code = 'https://github.com/x7c1/delta/pull/1（補足）';
    const { container } = render(
      <AssistantMarkdown
        text={`run \`${code}\` now\n\n\`\`\`\n${code}\n\`\`\`\n`}
      />,
    );
    const [inline, block] = container.querySelectorAll('code');
    expect(inline!.textContent).toBe(code);
    expect(block!.textContent).toBe(`${code}\n`);
    expectNoBoundary(container);
  });

  it('keeps an ASCII asterisk in the middle of a URL', () => {
    const url = 'https://web.archive.org/web/*/example.com';
    const { container } = render(<AssistantMarkdown text={`${url} です`} />);
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', url);
    expect(container.textContent).toBe(`${url} です`);
    expectNoBoundary(container);
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
