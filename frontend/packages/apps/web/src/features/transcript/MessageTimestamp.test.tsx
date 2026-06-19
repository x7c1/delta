import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MessageTimestamp } from './MessageTimestamp';

describe('MessageTimestamp', () => {
  it('renders the formatted timestamp text', () => {
    render(<MessageTimestamp timestamp="2026-01-01 09:00:00" />);
    expect(screen.getByText('2026-01-01 09:00:00')).toBeInTheDocument();
  });

  it('applies the canonical monospace base styling', () => {
    render(<MessageTimestamp timestamp="2026-01-01 09:00:00" />);
    const node = screen.getByText('2026-01-01 09:00:00');
    // The single source of timestamp styling: mono, xs, tabular-nums.
    expect(node).toHaveClass('font-mono');
    expect(node).toHaveClass('text-xs');
    expect(node).toHaveClass('tabular-nums');
  });

  it('merges a caller className alongside the base styling', () => {
    render(
      <MessageTimestamp timestamp="2026-01-01 09:00:00" className="mt-1" />,
    );
    const node = screen.getByText('2026-01-01 09:00:00');
    expect(node).toHaveClass('font-mono');
    expect(node).toHaveClass('mt-1');
  });

  it('forwards arbitrary span props to the rendered element', () => {
    render(
      <MessageTimestamp
        timestamp="2026-01-01 09:00:00"
        data-testid="ts"
        aria-label="when"
      />,
    );
    const node = screen.getByTestId('ts');
    expect(node).toHaveTextContent('2026-01-01 09:00:00');
    expect(node).toHaveAttribute('aria-label', 'when');
  });
});
