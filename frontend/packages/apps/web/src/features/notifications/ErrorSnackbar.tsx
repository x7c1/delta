import { useNotificationStore } from '../../store/notificationStore';

/**
 * The bottom-right stack of transient error notifications.
 *
 * Notifications live in {@link useNotificationStore}: any feature can push
 * one and it renders here. Each entry auto-dismisses after a few seconds
 * (see the store) and carries its own close button for immediate dismissal.
 *
 * The design is intentionally minimal — Delta has no toast infrastructure
 * yet, and this component exists to serve the `open cwd` error paths (VS
 * Code missing, path rejected, spawn failure). It is a thin surface that a
 * future toast system can absorb without changing the caller API
 * (`useNotificationStore.showError`).
 */
export function ErrorSnackbar() {
  const errors = useNotificationStore((state) => state.errors);
  const dismissError = useNotificationStore((state) => state.dismissError);

  if (errors.length === 0) {
    return null;
  }

  return (
    <div
      // Fixed to the bottom-right so it is visible from any pane without
      // covering the composer at the bottom-center. `z-50` sits above the
      // Menu dropdown's `z-10` and the transcript overlay's z-index so a
      // notification is never painted under a floating panel.
      className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 max-w-[90vw] flex-col gap-2"
      data-testid="error-snackbar"
    >
      {errors.map((notification) => (
        <div
          key={notification.id}
          // `role="alert"` reuses the pattern already used elsewhere in the
          // codebase (PermissionNotice, workdir picker) so screen readers
          // pick it up without extra ARIA plumbing.
          role="alert"
          className="pointer-events-auto flex items-start gap-2 rounded border border-danger/40 bg-surface-elevated px-3 py-2 text-caption shadow-lg"
          data-testid="error-snackbar-item"
        >
          <div className="min-w-0 flex-1">
            <p className="font-medium text-danger">{notification.title}</p>
            {notification.detail && (
              <p className="mt-0.5 break-words text-fg-muted">
                {notification.detail}
              </p>
            )}
          </div>
          <button
            type="button"
            aria-label="Dismiss notification"
            onClick={() => dismissError(notification.id)}
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
      ))}
    </div>
  );
}
