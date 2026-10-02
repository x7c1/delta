import { create } from 'zustand';
import type { SessionId } from '@delta/model';

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

/**
 * Something a notification offers to do next, rendered as a button beside its
 * text. A typed intent rather than a callback, so a store slice can attach one
 * without reaching into navigation, the queue stays plain data, and the
 * snackbar decides how each intent is carried out.
 *
 * - `open-session` — focus the named session, as picking its navigator card
 *   does. Offered when the notification is about a session the user may not
 *   be able to find on their own (see `reportUnwatchedSpawnFailure`).
 */
export type NotificationAction = { kind: 'open-session'; sessionId: SessionId };

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
  /** Optional next step, rendered as a button (see {@link NotificationAction}). */
  action?: NotificationAction;
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
 * It is also the only surface visible from every pane, which is why the
 * spawn-failure paths raise it: a launch that ends while the user is looking
 * at another session — or one this window never started — has no pane on
 * screen to explain itself in (see `reportUntrackedSpawnFailure` and
 * `reportUnwatchedSpawnFailure`).
 *
 * Kept isolated to this file (rather than folded into `useNavStore`)
 * because a global notification queue is a genuinely cross-cutting
 * concern — every feature can push to it — and nothing in nav state
 * depends on it. Zustand is the store library Delta already uses.
 */
export interface NotificationState {
  notifications: AppNotification[];
  /** Push a failure, returning its id for programmatic dismissal. */
  showError: (
    title: string,
    detail?: string,
    action?: NotificationAction,
  ) => number;
  /**
   * Push a plain statement of fact — an outcome the user asked for, and where
   * its aftermath went. Same queue and same auto-dismiss as
   * {@link NotificationState.showError}; only the tone differs.
   */
  showInfo: (
    title: string,
    detail?: string,
    action?: NotificationAction,
  ) => number;
  /** Dismiss a specific notification by id (no-op if it is already gone). */
  dismissNotification: (id: number) => void;
  /**
   * Dismiss every notification whose action opens this session. Called when
   * the session is removed: its Open would otherwise focus an id that no
   * longer exists, and the workspace would then move focus somewhere the user
   * never asked to go.
   */
  dismissSessionNotifications: (sessionId: SessionId) => void;
  /**
   * Keep a notification on screen past its auto-dismiss time while the user is
   * pointing at it or has keyboard focus inside it — so it does not vanish
   * under a reader, or from beneath a focused button. Paired with
   * {@link NotificationState.releaseNotification}.
   */
  holdNotification: (id: number) => void;
  /**
   * End a {@link NotificationState.holdNotification}. A notification whose
   * time ran out while it was held is dismissed now.
   */
  releaseNotification: (id: number) => void;
}

let nextNotificationId = 1;

export const useNotificationStore = create<NotificationState>((set, get) => {
  // Ids currently held on screen, and held ids whose time has run out. Plain
  // bookkeeping, not state: nothing renders from them, and an id is never
  // reused, so a stale entry is inert.
  const held = new Set<number>();
  const expiredWhileHeld = new Set<number>();

  const dismiss = (id: number) => {
    held.delete(id);
    expiredWhileHeld.delete(id);
    set((state) => ({
      notifications: state.notifications.filter((n) => n.id !== id),
    }));
  };

  const push = (
    tone: NotificationTone,
    title: string,
    detail?: string,
    action?: NotificationAction,
  ) => {
    const id = nextNotificationId++;
    set((state) => ({
      notifications: [
        ...state.notifications,
        { id, tone, title, detail, action },
      ],
    }));
    // Auto-dismiss so a stale click does not linger — unless the user is on
    // it, in which case it goes when they leave. Dismissing an entry the user
    // already closed is a no-op filter.
    if (typeof window !== 'undefined') {
      window.setTimeout(() => {
        if (held.has(id)) {
          expiredWhileHeld.add(id);
          return;
        }
        dismiss(id);
      }, AUTO_DISMISS_MS);
    }
    return id;
  };

  return {
    notifications: [],
    showError: (title, detail, action) => push('error', title, detail, action),
    showInfo: (title, detail, action) => push('info', title, detail, action),
    dismissNotification: dismiss,
    dismissSessionNotifications: (sessionId) => {
      for (const notification of get().notifications) {
        if (
          notification.action?.kind === 'open-session' &&
          notification.action.sessionId === sessionId
        ) {
          dismiss(notification.id);
        }
      }
    },
    holdNotification: (id) => {
      held.add(id);
    },
    releaseNotification: (id) => {
      held.delete(id);
      if (expiredWhileHeld.has(id)) {
        dismiss(id);
      }
    },
  };
});
