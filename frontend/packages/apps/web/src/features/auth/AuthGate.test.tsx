import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import { AuthGate } from './AuthGate';

afterEach(() => {
  window.history.replaceState(null, '', '/');
});

/** Change only the fragment, as pasting a link into the same tab does. */
function navigateToFragment(hash: string): void {
  act(() => {
    window.history.replaceState(null, '', `/${hash}`);
    window.dispatchEvent(new HashChangeEvent('hashchange'));
  });
}

describe('AuthGate', () => {
  it('renders the app while the page holds a token', () => {
    render(
      <AuthGate initial={{ kind: 'ready' }}>
        {() => <div data-testid="app" />}
      </AuthGate>,
    );
    expect(screen.getByTestId('app')).toBeInTheDocument();
    expect(screen.queryByTestId('reopen-screen')).not.toBeInTheDocument();
  });

  it('shows the re-open screen once the server answers 401', () => {
    let reportUnauthorized: () => void = () => {};
    render(
      <AuthGate initial={{ kind: 'ready' }}>
        {(onUnauthorized) => {
          reportUnauthorized = onUnauthorized;
          return <div data-testid="app" />;
        }}
      </AuthGate>,
    );

    act(() => reportUnauthorized());

    expect(screen.queryByTestId('app')).not.toBeInTheDocument();
    expect(screen.getByTestId('reopen-screen')).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent(/restarted/);
    expect(screen.getByTestId('reopen-command')).toHaveTextContent(
      `delta-server open-url --origin ${window.location.origin}`,
    );
  });

  it('shows the re-open screen when the page holds no token', () => {
    render(
      <AuthGate initial={{ kind: 'missing' }}>
        {() => <div data-testid="app" />}
      </AuthGate>,
    );
    expect(screen.queryByTestId('app')).not.toBeInTheDocument();
    expect(screen.getByTestId('reopen-screen')).toBeInTheDocument();
  });

  it('says a used link no longer works', () => {
    render(
      <AuthGate initial={{ kind: 'code-refused' }}>
        {() => <div data-testid="app" />}
      </AuthGate>,
    );
    expect(screen.getByRole('status')).toHaveTextContent(/already been used/);
  });

  it('reloads the page when a link with a new code is opened in the same tab', () => {
    const reload = vi.fn();
    render(
      <AuthGate initial={{ kind: 'code-refused' }} reload={reload}>
        {() => <div data-testid="app" />}
      </AuthGate>,
    );

    navigateToFragment('#end');
    expect(reload).not.toHaveBeenCalled();

    navigateToFragment('#code=fresh');
    expect(reload).toHaveBeenCalledTimes(1);
  });
});
