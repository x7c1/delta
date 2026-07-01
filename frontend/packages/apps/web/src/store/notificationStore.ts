import { create } from 'zustand';

/**
 * How long an auto-dismissing notification stays visible, in milliseconds.
 * The snackbar is a passing alert (the click that triggered the action has
 * already returned an error), not a stop-and-read modal — keep it long
 * enough to read a short phrase and short enough that a follow-up click
 * does not accumulate stale entries.
 */
const AUTO_DISMISS_MS = 6000;

/** One transient error notification presented in the app-wide snackbar. */
export interface ErrorNotification {
  /**
   * Stable id, unique for the notification's lifetime. Generated on push.
   * Used as the React list key and the dismiss handle so a specific entry
   * can be removed without touching the others.
   */
  id: number;
  /** Human-facing headline (e.g. "Could not open in VS Code"). */
  title: string;
  /**
   * Optional secondary line with a specific cause (e.g. "VS Code is not
   * installed"). Absent when the title alone is descriptive.
   */
  detail?: string;
}

/**
 * Ephemeral notification queue driving the app-wide error snackbar.
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
 * Kept isolated to this file (rather than folded into `useNavStore`)
 * because a global notification queue is a genuinely cross-cutting
 * concern — every feature can push to it — and nothing in nav state
 * depends on it. Zustand is the store library Delta already uses.
 */
export interface NotificationState {
  errors: ErrorNotification[];
  /** Push a new error, returning its id for programmatic dismissal. */
  showError: (title: string, detail?: string) => number;
  /** Dismiss a specific error by id (no-op if it is already gone). */
  dismissError: (id: number) => void;
}

let nextNotificationId = 1;

export const useNotificationStore = create<NotificationState>((set) => ({
  errors: [],
  showError: (title, detail) => {
    const id = nextNotificationId++;
    set((state) => ({
      errors: [...state.errors, { id, title, detail }],
    }));
    // Auto-dismiss so a stale click does not linger. Guarded by an
    // existence check because the user may have dismissed it explicitly
    // in the meantime.
    if (typeof window !== 'undefined') {
      window.setTimeout(() => {
        set((state) => ({
          errors: state.errors.filter((n) => n.id !== id),
        }));
      }, AUTO_DISMISS_MS);
    }
    return id;
  },
  dismissError: (id) => {
    set((state) => ({
      errors: state.errors.filter((n) => n.id !== id),
    }));
  },
}));
