import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { Collapsible } from './Collapsible';

describe('Collapsible', () => {
  it('hides the body until the summary is clicked', () => {
    render(
      <Collapsible summary="tool: Bash">
        <pre>ls -la</pre>
      </Collapsible>,
    );

    expect(screen.queryByText('ls -la')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /tool: Bash/ }));

    expect(screen.getByText('ls -la')).toBeInTheDocument();
    expect(screen.getByRole('button')).toHaveAttribute('aria-expanded', 'true');
  });
});
