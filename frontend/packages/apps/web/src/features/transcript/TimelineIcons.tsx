/**
 * Glyph for the collapsed "Thread" toggle button: a stylised activity / signal
 * trace (a polyline of small peaks) so the button reads as a timeline at a
 * glance. Mirrors {@link TerminalIcon} (in `WorkspaceScreen`) in size and
 * stroke weight so the two buttons sit visually balanced in the same row.
 * Decorative — always `aria-hidden`, so the button's accessible name stays
 * its "Thread" label.
 */
export function ThreadTimelineIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M3 12h3l3-7 4 14 3-7h5" />
    </svg>
  );
}

/**
 * Glyph for the jump-to-start button: a Lucide-style skip-back icon — a
 * left-pointing triangle with a short vertical bar pinned to the left edge,
 * so the shape reads as "rewind to the start". Mirrors
 * {@link ThreadTimelineIcon}'s stroke weight / line joins / viewBox so the
 * three header icons read as a coherent set, and is always `aria-hidden`
 * because the button carries its own accessible label.
 */
export function SkipBackIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <polygon points="19,20 9,12 19,4" />
      <line x1="5" y1="19" x2="5" y2="5" />
    </svg>
  );
}

/**
 * Glyph for the jump-to-end button: the mirror of {@link SkipBackIcon} — a
 * right-pointing triangle with a short vertical bar pinned to the right
 * edge, reading as "skip to the end". Same stroke / join / viewBox as the
 * other header icons.
 */
export function SkipForwardIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <polygon points="5,4 15,12 5,20" />
      <line x1="19" y1="5" x2="19" y2="19" />
    </svg>
  );
}
