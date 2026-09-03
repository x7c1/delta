import { create } from 'zustand';

/**
 * How long an auto-dismissing notification stays visible, in milliseconds.
 * The snackbar is a passing alert (the click that triggered the action has
 * already returned an error), not a stop-and-read modal — keep it long
 * enough to read a short phrase and short enough that a follow-up click
 * does not accumulate stale entries.
 */
const AUTO_DISMISS_MS = 6000;

/**
 * How a notification reads: something went wrong, or something the user asked
 * for happened and they should know where its aftermath went.
 *
 * The distinction is not decoration. Dressing an outcome the user requested —
 * cancelling a launch that was still starting — in error colours tells them
 * their action broke something, which is the one thing it did not do.
 */
export type NotificationTone = 'error' | 'info';

/** One transient notification presented in the app-wide snackbar. */
export interface AppNotification {
  /**
   * Stable id, unique for the notification's lifetime. Generated on push.
   * Used as the React list key and the dismiss handle so a specific entry
   * can be removed without touching the others.
   */
  id: number;
  tone: NotificationTone;
  /** Human-facing headline (e.g. "Could not open in VS Code"). */
  title: string;
  /**
   * Optional secondary line with a specific cause (e.g. "VS Code is not
   * installed"). Absent when the title alone is descriptive.
   */
  detail?: string;
}

/**
 * Ephemeral notification queue driving the app-wide snackbar.
 *
 * Delta has no toast/snackbar infrastructure at all today — the closest
 * existing patterns are inline `role="alert"` messages next to a failing
 * form field or overlay. Those work when the click site itself sticks
 * around after the failure, but neither `open cwd` entry point has that
 * property: the click closes the session menu / navigates the pointer
 * away from the message meta line, so an inline error has nowhere to
 * live. A tiny queued snackbar is the minimum surface that fits both
 * click sites without inventing a heavy pattern.
 *
 * It is also the only surface that outlives a session, which is why the
 * spawn-failure paths raise it: the session whose launch ended is being
 * deleted as the news arrives, so a notice rendered inside its pane has
 * nowhere to live either (see `reportUntrackedSpawnFailure`).
 *
 * Kept isolated to this file (rather than folded into `useNavStore`)
 * because a global notification queue is a genuinely cross-cutting
 * concern — every feature can push to it — and nothing in nav state
 * depends on it. Zustand is the store library Delta already uses.
 */
export interface NotificationState {
  notifications: AppNotification[];
  /** Push a failure, returning its id for programmatic dismissal. */
  showError: (title: string, detail?: string) => number;
  /**
   * Push a plain statement of fact — an outcome the user asked for, and where
   * its aftermath went. Same queue and same auto-dismiss as
   * {@link NotificationState.showError}; only the tone differs.
   */
  showInfo: (title: string, detail?: string) => number;
  /** Dismiss a specific notification by id (no-op if it is already gone). */
  dismissNotification: (id: number) => void;
}

let nextNotificationId = 1;

export const useNotificationStore = create<NotificationState>((set) => {
  const push = (tone: NotificationTone, title: string, detail?: string) => {
    const id = nextNotificationId++;
    set((state) => ({
      notifications: [...state.notifications, { id, tone, title, detail }],
    }));
    // Auto-dismiss so a stale click does not linger. Guarded by an
    // existence check because the user may have dismissed it explicitly
    // in the meantime.
    if (typeof window !== 'undefined') {
      window.setTimeout(() => {
        set((state) => ({
          notifications: state.notifications.filter((n) => n.id !== id),
        }));
      }, AUTO_DISMISS_MS);
    }
    return id;
  };

  return {
    notifications: [],
    showError: (title, detail) => push('error', title, detail),
    showInfo: (title, detail) => push('info', title, detail),
    dismissNotification: (id) => {
      set((state) => ({
        notifications: state.notifications.filter((n) => n.id !== id),
      }));
    },
  };
});
