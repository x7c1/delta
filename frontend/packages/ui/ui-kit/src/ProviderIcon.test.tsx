import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import claudeMark from '@lobehub/icons-static-svg/icons/claude.svg';
import codexMark from '@lobehub/icons-static-svg/icons/codex.svg';
import { ProviderIcon } from './ProviderIcon';

/**
 * The vitest setup runs in jsdom with no layout and no stylesheet, so neither
 * the painted glyph nor its resolved color can be asserted here. These tests
 * lock in the contract that survives that: each provider masks in its OWN brand
 * mark, the accessible name is the full product name, the glyph itself is
 * hidden from assistive tech, and the icon is color-inheriting — no provider
 * accent utility anywhere in the markup.
 */

/** The masked glyph inside the labelled wrapper. */
function glyphOf(wrapper: HTMLElement): HTMLElement {
  const glyph = wrapper.firstElementChild;
  if (!(glyph instanceof HTMLElement)) {
    throw new Error('ProviderIcon rendered no glyph element');
  }
  return glyph;
}

describe('ProviderIcon', () => {
  it('labels the Claude icon with the full product name', () => {
    render(<ProviderIcon provider="claude" />);

    const icon = screen.getByTitle('Claude Code');
    expect(icon).toHaveAttribute('role', 'img');
    expect(icon).toHaveAttribute('aria-label', 'Claude Code');
    // The glyph carries no accessible name of its own: the wrapper's aria-label
    // is what screen readers announce.
    expect(glyphOf(icon)).toHaveAttribute('aria-hidden', 'true');
  });

  it('labels the Codex icon with the full product name', () => {
    render(<ProviderIcon provider="codex" />);

    const icon = screen.getByTitle('Codex');
    expect(icon).toHaveAttribute('role', 'img');
    expect(icon).toHaveAttribute('aria-label', 'Codex');
  });

  it("masks in each provider's own brand mark", () => {
    // The two providers must be distinguishable by their glyph alone — there is
    // no color and no lettering to fall back on — and each must mask in ITS OWN
    // file: swapping the two entries of the icon map shows every Codex session
    // the Claude spark, and no other layer would notice (the e2e asserts the
    // accessible name and the painted color, never which glyph is cut out).
    // Comparing against the imported URL keeps that check bundler-agnostic —
    // both sides resolve the same way whether the asset ends up a file path or
    // an inlined `data:` URI — and pins the double quotes the URL must be
    // wrapped in (see `maskStyle`).
    const { rerender } = render(<ProviderIcon provider="claude" />);
    expect(glyphOf(screen.getByTitle('Claude Code')).style.maskImage).toBe(
      `url("${claudeMark}")`,
    );
    rerender(<ProviderIcon provider="codex" />);
    expect(glyphOf(screen.getByTitle('Codex')).style.maskImage).toBe(
      `url("${codexMark}")`,
    );
  });

  it('inherits the surrounding text color, with no provider accent', () => {
    // Unlike ProviderBadge, this icon is deliberately colorless: it paints in
    // `currentColor` so a caller's `text-*` utility (e.g. the session card's
    // `text-fg-subtle` meta line) decides the tone. Any `text-provider-*` /
    // `bg-provider-*` utility would break that.
    const { container } = render(<ProviderIcon provider="claude" />);

    expect(container.innerHTML).not.toMatch(/(text|bg)-provider-/);
    expect(glyphOf(screen.getByTitle('Claude Code')).className).toContain(
      'bg-current',
    );
  });

  it('appends caller classes without dropping its own sizing', () => {
    render(<ProviderIcon provider="codex" className="text-fg-subtle" />);

    const icon = screen.getByTitle('Codex');
    expect(icon.className).toContain('text-fg-subtle');
    // Sized to the surrounding line: the marks are 1em squares.
    expect(icon.className).toContain('h-[1em]');
    expect(icon.className).toContain('w-[1em]');
  });
});
