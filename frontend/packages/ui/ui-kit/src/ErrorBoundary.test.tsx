import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ErrorBoundary } from './ErrorBoundary';

function Boom(): never {
  throw new Error('kaboom');
}

describe('ErrorBoundary', () => {
  beforeEach(() => {
    // React logs caught errors to the console; silence the expected noise so the
    // test output stays readable without hiding real failures.
    vi.spyOn(console, 'error').mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('shows the fallback instead of unmounting when a child throws', () => {
    render(
      <ErrorBoundary fallback={(error) => <p>caught: {error.message}</p>}>
        <Boom />
      </ErrorBoundary>,
    );

    expect(screen.getByText('caught: kaboom')).toBeInTheDocument();
  });

  it('renders children normally when nothing throws', () => {
    render(
      <ErrorBoundary fallback={() => <p>fallback</p>}>
        <p>healthy</p>
      </ErrorBoundary>,
    );

    expect(screen.getByText('healthy')).toBeInTheDocument();
    expect(screen.queryByText('fallback')).not.toBeInTheDocument();
  });

  it('clears the error and retries the subtree when resetKey changes', () => {
    const { rerender } = render(
      <ErrorBoundary resetKey="a" fallback={() => <p>fallback</p>}>
        <Boom />
      </ErrorBoundary>,
    );

    expect(screen.getByText('fallback')).toBeInTheDocument();

    rerender(
      <ErrorBoundary resetKey="b" fallback={() => <p>fallback</p>}>
        <p>recovered</p>
      </ErrorBoundary>,
    );

    expect(screen.getByText('recovered')).toBeInTheDocument();
    expect(screen.queryByText('fallback')).not.toBeInTheDocument();
  });
});
