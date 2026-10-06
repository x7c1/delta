import { useEffect, useState } from 'react';
import { Button } from '@delta/ui-kit';
import { exactBytes, formatBytes } from './formatBytes';

/**
 * The pieces every Storage block draws a file or directory with, so a path
 * reads the same in the inventory, the worktree list and the snapshot list.
 */

/**
 * A path in monospace, truncated from the left on overflow so the file name —
 * the part that tells two paths apart — stays visible. The `rtl` container
 * puts the ellipsis at the start; the `<bdi>` keeps the path itself reading
 * left to right, leading `/` included.
 */
export function PathText({ value }: { value: string }) {
  return (
    <span
      className="min-w-0 flex-1 truncate text-left font-mono text-code text-fg [direction:rtl]"
      title={value}
    >
      <bdi>{value}</bdi>
    </span>
  );
}

/** A byte count humanised, with the exact count in a tooltip. */
export function Size({ bytes }: { bytes: number }) {
  return (
    <span
      className="shrink-0 text-caption tabular-nums text-fg-subtle"
      title={exactBytes(bytes)}
    >
      {formatBytes(bytes)}
    </span>
  );
}

/** How long the copy control says "Copied" (or "Copy failed") after a click. */
const COPY_FEEDBACK_MS = 1500;

type CopyState = 'idle' | 'copied' | 'failed';

export function CopyButton({ value, label }: { value: string; label: string }) {
  const [state, setState] = useState<CopyState>('idle');

  useEffect(() => {
    if (state === 'idle') {
      return;
    }
    const timer = window.setTimeout(() => setState('idle'), COPY_FEEDBACK_MS);
    return () => window.clearTimeout(timer);
  }, [state]);

  // Started inside a promise so a webview without `navigator.clipboard`, where
  // the call throws synchronously, also ends in "Copy failed" rather than no change.
  const copy = () => {
    Promise.resolve()
      .then(() => navigator.clipboard.writeText(value))
      .then(
        () => setState('copied'),
        () => setState('failed'),
      );
  };

  return (
    <Button
      size="sm"
      variant="ghost"
      className="shrink-0"
      onClick={copy}
      aria-label={`Copy ${label}`}
    >
      {state === 'copied' ? 'Copied' : state === 'failed' ? 'Copy failed' : 'Copy'}
    </Button>
  );
}

/** The final component of `path` (`/a/b/delta.db.bak-v3` → `delta.db.bak-v3`). */
export function baseName(path: string): string {
  const trimmed = path.replace(/\/+$/, '');
  return trimmed.slice(trimmed.lastIndexOf('/') + 1);
}

/** The message of a failed request, for a line under the control that made it. */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : 'The request failed.';
}
