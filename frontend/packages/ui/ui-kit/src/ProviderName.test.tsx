import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ProviderName } from './ProviderName';

describe('ProviderName', () => {
  it('writes the Claude product name in the Claude hue', () => {
    render(<ProviderName provider="claude" />);

    const name = screen.getByText('Claude Code');
    expect(name.className).toContain('text-provider-claude');
    expect(name.className).not.toContain('text-provider-codex');
  });

  it('writes the Codex product name in the Codex hue', () => {
    render(<ProviderName provider="codex" />);

    const name = screen.getByText('Codex');
    expect(name.className).toContain('text-provider-codex');
    expect(name.className).not.toContain('text-provider-claude');
  });

  it('merges a caller className onto the name', () => {
    render(<ProviderName provider="claude" className="font-medium" />);

    expect(screen.getByText('Claude Code').className).toContain('font-medium');
  });
});
