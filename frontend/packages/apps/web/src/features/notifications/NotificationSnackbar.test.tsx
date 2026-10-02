import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { useNavStore } from '../../store/navStore';
import { useNotificationStore } from '../../store/notificationStore';
import { NotificationSnackbar } from './NotificationSnackbar';

/** Matches the store's auto-dismiss delay, with a little to spare. */
const PAST_AUTO_DISMISS_MS = 6100;

describe('NotificationSnackbar', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useNotificationStore.setState({ notifications: [] });
    useNavStore.setState({
      focusedSessionId: 'sess-other',
      settingsOpen: false,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  function raiseFailure() {
    act(() => {
      useNotificationStore
        .getState()
        .showError(
          'Session “Fix the flaky login test” failed to start',
          'git error',
          { kind: 'open-session', sessionId: 'sess-failed' },
        );
    });
  }

  it('opens the session a notification names and dismisses it', () => {
    render(<NotificationSnackbar />);
    raiseFailure();

    const open = screen.getByRole('button', { name: 'Open the session' });
    expect(open).toHaveTextContent('Open');
    fireEvent.click(open);

    // The same focus change as picking the session's navigator card.
    expect(useNavStore.getState().focusedSessionId).toBe('sess-failed');
    expect(useNotificationStore.getState().notifications).toEqual([]);
    expect(screen.queryByTestId('notification-snackbar')).toBeNull();
  });

  it('offers no action when the notification has none', () => {
    render(<NotificationSnackbar />);
    act(() => {
      useNotificationStore
        .getState()
        .showError('Could not open in VS Code', 'VS Code is not installed');
    });

    expect(screen.getByRole('alert')).toHaveTextContent(
      'Could not open in VS Code',
    );
    expect(
      screen.getAllByRole('button').map((b) => b.getAttribute('aria-label')),
    ).toEqual(['Dismiss notification']);
  });

  it('auto-dismisses a notification the user is not on', () => {
    render(<NotificationSnackbar />);
    raiseFailure();

    act(() => {
      vi.advanceTimersByTime(PAST_AUTO_DISMISS_MS);
    });

    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('keeps a notification while focus is in it, until focus leaves', () => {
    render(<NotificationSnackbar />);
    raiseFailure();

    act(() => {
      screen.getByRole('button', { name: 'Open the session' }).focus();
    });
    act(() => {
      vi.advanceTimersByTime(PAST_AUTO_DISMISS_MS);
    });
    expect(screen.getByRole('alert')).toBeInTheDocument();

    // Tabbing to the entry's own close button is still focus within it.
    act(() => {
      screen.getByRole('button', { name: 'Dismiss notification' }).focus();
    });
    expect(screen.getByRole('alert')).toBeInTheDocument();

    act(() => {
      (document.activeElement as HTMLElement).blur();
    });
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('keeps a notification while it is hovered, until the pointer leaves', () => {
    render(<NotificationSnackbar />);
    raiseFailure();

    const item = screen.getByRole('alert');
    fireEvent.mouseEnter(item);
    act(() => {
      vi.advanceTimersByTime(PAST_AUTO_DISMISS_MS);
    });
    expect(screen.getByRole('alert')).toBeInTheDocument();

    fireEvent.mouseLeave(item);
    expect(screen.queryByRole('alert')).toBeNull();
  });
});
