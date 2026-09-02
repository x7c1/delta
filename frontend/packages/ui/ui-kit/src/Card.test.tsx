import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Card } from './Card';

describe('Card', () => {
  it('shows the body without any click, and offers nothing to click', () => {
    render(
      <Card summary="thinking" testId="card">
        <pre>let me reason</pre>
      </Card>,
    );

    expect(screen.getByText('thinking')).toBeInTheDocument();
    expect(screen.getByText('let me reason')).toBeInTheDocument();
    expect(
      screen.getByTestId('card').querySelector('button'),
    ).toBeNull();
    expect(screen.queryByRole('button')).toBeNull();
  });
});
