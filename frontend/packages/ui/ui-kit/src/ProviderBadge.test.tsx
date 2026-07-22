import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ProviderBadge } from './ProviderBadge';

/**
 * The vitest setup runs in jsdom with the stylesheet unloaded, so the accent
 * *color* itself cannot be asserted here (a `getComputedStyle` lookup would be
 * empty regardless of class). These tests lock in the contract that survives
 * that: the monogram text, the theme-token utility class that carries the
 * accent, and the full-name tooltip / accessible name. The rendered colors are
 * covered by the e2e-fake layer, where the real stylesheet is loaded.
 */
describe('ProviderBadge', () => {
  it('renders the Claude monogram, accent class, and full name', () => {
    render(<ProviderBadge provider="claude" />);

    const badge = screen.getByTitle('Claude Code');
    expect(badge).toHaveTextContent('CL');
    // The accent hue is carried by the provider token utility (foreground +
    // low-alpha wash), sourced from the active theme block.
    expect(badge.className).toContain('text-provider-claude');
    expect(badge.className).toContain('bg-provider-claude/15');
    // The two-letter monogram is decorative; the accessible name is the full
    // product name so screen readers announce "Claude Code", not "CL".
    expect(badge).toHaveAttribute('aria-label', 'Claude Code');
  });

  it('renders the Codex monogram, accent class, and full name', () => {
    render(<ProviderBadge provider="codex" />);

    const badge = screen.getByTitle('Codex');
    expect(badge).toHaveTextContent('CX');
    expect(badge.className).toContain('text-provider-codex');
    expect(badge.className).toContain('bg-provider-codex/15');
    expect(badge).toHaveAttribute('aria-label', 'Codex');
  });

  it('distinguishes the two providers by their monogram alone (color-independent)', () => {
    // The letters must carry the distinction on their own — color is only a
    // redundant reinforcement — so the two monograms are never equal.
    const { rerender } = render(<ProviderBadge provider="claude" />);
    const claude = screen.getByTitle('Claude Code').textContent;
    rerender(<ProviderBadge provider="codex" />);
    const codex = screen.getByTitle('Codex').textContent;
    expect(claude).not.toBe(codex);
  });
});
