import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { StatusDot } from './StatusDot';

describe('StatusDot', () => {
  it('exposes a dot-only indicator via its title as the accessible name', () => {
    render(<StatusDot tone="green" title="Server connection: connected" />);

    const indicator = screen.getByRole('status', {
      name: 'Server connection: connected',
    });
    expect(indicator).toHaveAttribute('title', 'Server connection: connected');
  });

  it('keeps the visible label as the name and does not double up aria-label', () => {
    render(<StatusDot tone="slate" label="closed" title="Closed" />);

    // The visible text is the accessible name; no separate status role/name.
    expect(screen.getByText('closed')).toBeInTheDocument();
    expect(
      screen.queryByRole('status', { name: 'Closed' }),
    ).not.toBeInTheDocument();
  });
});
