import type { ThreadId } from './ids';

/**
 * Minimal shape needed to derive a thread's display name. The wire `Thread`
 * type satisfies it; keeping the helper structurally typed avoids dragging a
 * dependency on `@delta/wire-gen` into this dep-free package.
 */
export interface ThreadNamed {
  id: ThreadId;
  title: string;
}

/**
 * Display name shown for the session's main (root) thread. Mirrors the wording
 * Navigator uses when surfacing the main lane elsewhere (e.g. the transcript
 * breadcrumb) so the user sees a single name everywhere.
 */
export const MAIN_THREAD_DISPLAY_NAME = 'main';

/**
 * Fallback label when a thread's wire-side `title` is empty (a server-side
 * race that should not normally happen, but is recoverable in the UI). Keeps
 * the lane recognisable by id so the row is still selectable.
 */
export function emptyTitleFallback(threadId: ThreadId): string {
  return `thread ${threadId}`;
}

/**
 * Canonical display name for a thread.
 *
 * Returns the server-supplied `title` verbatim, which is the single source of
 * truth the Navigator tree (`ThreadTree`) and the transcript breadcrumb both
 * render. The timeline footer goes through the same helper so a subthread
 * cannot show two different names in two different panes.
 *
 * - `isMain = true` overrides the title with {@link MAIN_THREAD_DISPLAY_NAME}.
 *   The wire `title` of the main thread is typically the session's prompt
 *   itself, which is far too long to read as a lane label; the Navigator hides
 *   the main row entirely, so the timeline picks the conventional `"main"` to
 *   stay consistent with the breadcrumb's left-most crumb.
 * - When the title is empty after trimming, fall back to a stable
 *   `thread <id>` label so the row is still selectable.
 *
 * Truncation is intentionally NOT applied here — the caller (Navigator uses a
 * CSS `truncate`, the timeline reserves a fixed label column) is the right
 * place to decide how much fits. The full title is always available for a
 * tooltip; see {@link threadTooltip}.
 */
export function threadDisplayName(
  thread: ThreadNamed,
  options: { isMain?: boolean } = {},
): string {
  if (options.isMain === true) {
    return MAIN_THREAD_DISPLAY_NAME;
  }
  const trimmed = thread.title.trim();
  if (trimmed === '') {
    return emptyTitleFallback(thread.id);
  }
  return trimmed;
}

/**
 * Tooltip text for the lane / row. Mirrors {@link threadDisplayName}'s value
 * for the main thread (the conventional `"main"` reads cleanly in either
 * place), and exposes the full untrimmed title for everything else so a
 * caller that truncates visually can still surface the rest on hover.
 */
export function threadTooltip(
  thread: ThreadNamed,
  options: { isMain?: boolean } = {},
): string {
  if (options.isMain === true) {
    return MAIN_THREAD_DISPLAY_NAME;
  }
  const trimmed = thread.title.trim();
  if (trimmed === '') {
    return emptyTitleFallback(thread.id);
  }
  return trimmed;
}
