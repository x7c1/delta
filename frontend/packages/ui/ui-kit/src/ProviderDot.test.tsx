import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ProviderDot } from './ProviderDot';

describe('ProviderDot', () => {
  it('carries the Claude hue and the full product name', () => {
    render(<ProviderDot provider="claude" />);

    const dot = screen.getByRole('img', { name: 'Claude Code' });
    expect(dot).toHaveAttribute('title', 'Claude Code');
    expect(dot.className).toContain('bg-provider-claude');
    expect(dot.className).not.toContain('bg-provider-codex');
  });

  it('carries the Codex hue and the full product name', () => {
    render(<ProviderDot provider="codex" />);

    const dot = screen.getByRole('img', { name: 'Codex' });
    expect(dot).toHaveAttribute('title', 'Codex');
    expect(dot.className).toContain('bg-provider-codex');
    expect(dot.className).not.toContain('bg-provider-claude');
  });

  it('merges a caller className onto the dot', () => {
    render(<ProviderDot provider="claude" className="mt-1" />);

    expect(
      screen.getByRole('img', { name: 'Claude Code' }).className,
    ).toContain('mt-1');
  });
});
