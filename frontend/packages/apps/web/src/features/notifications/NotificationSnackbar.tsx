import { useEffect, useState } from 'react';
import { cn } from '@delta/ui-kit';
import { useNavStore } from '../../store/navStore';
import {
  useNotificationStore,
  type AppNotification,
  type NotificationAction,
} from '../../store/notificationStore';

/**
 * The bottom-right stack of transient notifications.
 *
 * Notifications live in {@link useNotificationStore}: any feature can push
 * one and it renders here. Each entry auto-dismisses after a few seconds
 * (see the store) — but not while the pointer is over it or keyboard focus is
 * inside it — and carries its own close button for immediate dismissal.
 * An entry's `tone` picks its colour — a failure is danger,
 * a plain statement of fact is not (see `NotificationTone` in the store).
 * An entry may also offer one next step (see `NotificationAction`), which
 * this component carries out and then dismisses the entry.
 *
 * The design is intentionally minimal — Delta has no toast infrastructure
 * yet, and this component exists to serve the `open cwd` error paths (VS
 * Code missing, path rejected, spawn failure) and the launch outcomes the
 * user is not looking at. It is a thin surface that a future toast system can
 * absorb without changing the caller API.
 */
export function NotificationSnackbar() {
  const notifications = useNotificationStore((state) => state.notifications);

  if (notifications.length === 0) {
    return null;
  }

  return (
    <div
      // Fixed to the bottom-right so it is visible from any pane without
      // covering the composer at the bottom-center. `z-50` sits above the
      // Menu dropdown's `z-10` and the transcript overlay's z-index so a
      // notification is never painted under a floating panel.
      className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 max-w-[90vw] flex-col gap-2"
      data-testid="notification-snackbar"
    >
      {notifications.map((notification) => (
        <NotificationItem key={notification.id} notification={notification} />
      ))}
    </div>
  );
}

/** What an action's button reads, visibly and to a screen reader. */
function actionLabels(action: NotificationAction): {
  text: string;
  ariaLabel: string;
} {
  switch (action.kind) {
    case 'open-session':
      return { text: 'Open', ariaLabel: 'Open the session' };
  }
}

function NotificationItem({ notification }: { notification: AppNotification }) {
  const dismissNotification = useNotificationStore(
    (state) => state.dismissNotification,
  );
  const holdNotification = useNotificationStore(
    (state) => state.holdNotification,
  );
  const releaseNotification = useNotificationStore(
    (state) => state.releaseNotification,
  );
  const setFocusedSession = useNavStore((state) => state.setFocusedSession);
  const [hovered, setHovered] = useState(false);
  const [focusWithin, setFocusWithin] = useState(false);
  const held = hovered || focusWithin;

  // While the user is on the entry, it outlives its auto-dismiss time (see
  // `holdNotification`).
  useEffect(() => {
    if (!held) {
      return;
    }
    holdNotification(notification.id);
    return () => releaseNotification(notification.id);
  }, [held, notification.id, holdNotification, releaseNotification]);

  const runAction = (intent: NotificationAction) => {
    switch (intent.kind) {
      case 'open-session':
        // The same user navigation as picking the session's navigator card.
        setFocusedSession(intent.sessionId);
        break;
    }
    dismissNotification(notification.id);
  };

  const { action } = notification;
  const labels = action && actionLabels(action);

  return (
    <div
      // `role="alert"` reuses the pattern already used elsewhere in the
      // codebase (PermissionNotice, workdir picker) so screen readers
      // pick it up without extra ARIA plumbing.
      role="alert"
      className={cn(
        'pointer-events-auto flex items-start gap-2 rounded border bg-surface-elevated px-3 py-2 text-caption shadow-lg',
        notification.tone === 'error'
          ? 'border-danger/40'
          : 'border-border-default',
      )}
      data-testid="notification-snackbar-item"
      data-tone={notification.tone}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onFocus={() => setFocusWithin(true)}
      onBlur={(event) => {
        // Moving between this entry's own buttons is still focus within it.
        if (!event.currentTarget.contains(event.relatedTarget)) {
          setFocusWithin(false);
        }
      }}
    >
      <div className="min-w-0 flex-1">
        <p
          className={cn(
            'font-medium',
            notification.tone === 'error' ? 'text-danger' : 'text-fg',
          )}
        >
          {notification.title}
        </p>
        {notification.detail && (
          <p className="mt-0.5 break-words text-fg-muted">
            {notification.detail}
          </p>
        )}
      </div>
      {action && labels && (
        <button
          type="button"
          aria-label={labels.ariaLabel}
          onClick={() => runAction(action)}
          className="shrink-0 rounded px-1.5 py-0.5 font-medium text-fg hover:bg-surface-sunken"
        >
          {labels.text}
        </button>
      )}
      <button
        type="button"
        aria-label="Dismiss notification"
        onClick={() => dismissNotification(notification.id)}
        className="shrink-0 rounded p-0.5 text-fg-subtle hover:bg-surface-sunken hover:text-fg"
      >
        {/* Simple close glyph; inline so no icon-font dependency. */}
        <svg
          width="12"
          height="12"
          viewBox="0 0 12 12"
          fill="currentColor"
          aria-hidden="true"
        >
          <path d="M2.5 2.5 L9.5 9.5 M9.5 2.5 L2.5 9.5" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" />
        </svg>
      </button>
    </div>
  );
}
