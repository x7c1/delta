import { useState } from 'react';
import {
  usePrunePreviewQuery,
  usePruneSessionsMutation,
} from '@delta/api-client';
import type {
  KeptItem,
  PruneSessionsResponse,
  PruneStatus,
  SkippedSession,
} from '@delta/wire-gen';
import { Button } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { ConfirmPanel } from './ConfirmPanel';
import { errorMessage } from './StorageParts';

/** The age the block starts from, in days. */
const DEFAULT_DAYS = 30;

/** The status choices, each with the noun the preview line counts. */
const STATUS_CHOICES = {
  both: { label: 'Ended and failed', statuses: ['ended', 'failed'], noun: 'closed session' },
  ended: { label: 'Ended', statuses: ['ended'], noun: 'ended session' },
  failed: { label: 'Failed', statuses: ['failed'], noun: 'failed session' },
} as const satisfies Record<
  string,
  { label: string; statuses: readonly PruneStatus[]; noun: string }
>;

type StatusChoice = keyof typeof STATUS_CHOICES;

/** `1 closed session`, `12 closed sessions`. */
function countOf(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? '' : 's'}`;
}

/** The age part of the preview line: nothing extra for zero days. */
function ageOf(days: number): string {
  return days === 0 ? '' : ` older than ${days} day${days === 1 ? '' : 's'}`;
}

/**
 * The days typed into the input as a whole number of days, or `null` while
 * it is not one (empty, negative, fractional).
 */
function parseDays(text: string): number | null {
  if (!/^\d+$/.test(text.trim())) {
    return null;
  }
  return Number(text.trim());
}

/**
 * Storage block: remove old closed sessions in bulk.
 *
 * The user picks an age and which closed sessions (ended, failed, or both);
 * a live preview line says how many that is right now, the Remove button
 * asks for a confirmation naming the count, and the result says how many went,
 * how many were skipped, and which worktrees and branches stayed on disk and
 * why. Each session goes through the same removal the navigator's `Remove`
 * does, so anything holding work is kept. `active` gates the preview's fetch
 * to while the Storage category is shown.
 */
export function PruneSessionsBlock({ active }: { active: boolean }) {
  const client = useApiClient();
  const [daysText, setDaysText] = useState(String(DEFAULT_DAYS));
  const [choice, setChoice] = useState<StatusChoice>('both');
  const [confirming, setConfirming] = useState(false);
  const prune = usePruneSessionsMutation(client);

  const days = parseDays(daysText);
  const { statuses, noun } = STATUS_CHOICES[choice];
  const criteria = { older_than_days: days ?? 0, statuses: [...statuses] };
  const preview = usePrunePreviewQuery(client, criteria, active && days !== null);
  const count = preview.data?.count ?? 0;

  const changeCriteria = (apply: () => void) => {
    apply();
    setConfirming(false);
  };

  const confirm = () => {
    prune.mutate(criteria, { onSettled: () => setConfirming(false) });
  };

  return (
    <section className="flex flex-col gap-2" data-testid="storage-prune">
      <div>
        <h4 className="text-secondary font-medium text-fg">Remove old sessions</h4>
        <p className="text-caption text-fg-muted">
          Removes closed sessions whose last activity is at least this old, each
          as the session&apos;s own Remove does: a worktree with uncommitted work
          or an unmerged branch is kept.
        </p>
      </div>
      <div className="flex flex-wrap items-center gap-2 text-caption text-fg-muted">
        <label className="flex items-center gap-1">
          Older than
          <input
            type="number"
            min={0}
            step={1}
            inputMode="numeric"
            value={daysText}
            onChange={(event) => changeCriteria(() => setDaysText(event.target.value))}
            aria-label="Days since last activity"
            className="w-16 rounded border border-border-default bg-surface px-2 py-1 text-secondary text-fg focus:border-accent-hover focus:outline-none"
          />
          days
        </label>
        <label className="flex items-center gap-1">
          Sessions
          <select
            value={choice}
            onChange={(event) =>
              changeCriteria(() => setChoice(event.target.value as StatusChoice))
            }
            aria-label="Which closed sessions"
            className="rounded border border-border-default bg-surface px-2 py-1 text-secondary text-fg focus:border-accent-hover focus:outline-none"
          >
            {Object.entries(STATUS_CHOICES).map(([value, { label }]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className="text-caption text-fg" data-testid="storage-prune-preview">
        {days === null
          ? 'Enter a whole number of days.'
          : preview.isError
            ? 'Could not count the sessions.'
            : preview.isPending
              ? 'Counting…'
              : `${countOf(count, noun)}${ageOf(days)}.`}
      </p>
      {confirming ? (
        <ConfirmPanel
          confirmLabel={`Remove ${countOf(count, 'session')}`}
          onConfirm={confirm}
          onCancel={() => setConfirming(false)}
          pending={prune.isPending}
          testId="storage-prune-confirm"
        >
          <p>
            Remove {countOf(count, noun)}
            {ageOf(days ?? 0)} from Delta? Their conversations go for good;
            worktrees and branches holding work stay on disk.
          </p>
        </ConfirmPanel>
      ) : (
        <div>
          <Button
            size="sm"
            onClick={() => {
              prune.reset();
              setConfirming(true);
            }}
            disabled={days === null || !preview.isSuccess || count === 0}
          >
            Remove…
          </Button>
        </div>
      )}
      {prune.isError && (
        <p className="text-caption text-danger" role="alert">
          Could not remove the sessions: {errorMessage(prune.error)}
        </p>
      )}
      {prune.data && <PruneResult result={prune.data} />}
    </section>
  );
}

/** What a kept item's reason says, after "kept because". */
const KEEP_REASON_TEXT: Record<KeptItem['reason'], string> = {
  dirty: 'it has uncommitted or untracked files',
  unmerged: 'it is not merged',
  not_created_by_delta: 'Delta did not create it',
  worktree_kept: 'its worktree was kept',
  in_use_by_another_session: 'another session works in it',
  failed: 'removing it failed',
};

const KIND_TEXT: Record<KeptItem['kind'], string> = {
  worktree: 'Worktree',
  branch: 'Branch',
  trust_entry: 'Trust entry for',
};

const SKIP_REASON_TEXT: Record<SkippedSession['reason'], string> = {
  open: 'it is open',
  starting: 'it is still starting',
  gone: 'it was already removed',
  failed: 'removing it failed',
};

/** A reason with the server's detail appended, when it carried one. */
function withDetail(text: string, detail: string | null): string {
  return detail ? `${text}: ${detail}` : text;
}

/** The outcome of a bulk removal: counts, then what was skipped and kept. */
function PruneResult({ result }: { result: PruneSessionsResponse }) {
  return (
    <div
      className="flex flex-col gap-1 rounded border border-border-default px-3 py-2 text-caption text-fg"
      data-testid="storage-prune-result"
      role="status"
    >
      <p>
        Removed {countOf(result.removed, 'session')}
        {result.skipped.length > 0 && `; skipped ${result.skipped.length}`}.
      </p>
      {result.skipped.length > 0 && (
        <ul className="flex flex-col gap-0.5 text-fg-muted" data-testid="storage-prune-skipped">
          {result.skipped.map((skipped) => (
            <li key={skipped.session_id}>
              Skipped <span className="font-mono">{skipped.session_id}</span>:{' '}
              {withDetail(SKIP_REASON_TEXT[skipped.reason], skipped.detail)}
            </li>
          ))}
        </ul>
      )}
      {result.kept.length > 0 && (
        <>
          <p className="text-fg-muted">Kept on disk:</p>
          <ul className="flex flex-col gap-0.5 text-fg-muted" data-testid="storage-prune-kept">
            {result.kept.map((kept) => (
              <li key={`${kept.session_id}:${kept.kind}:${kept.target}`}>
                {KIND_TEXT[kept.kind]} <span className="font-mono">{kept.target}</span>,
                because {withDetail(KEEP_REASON_TEXT[kept.reason], kept.detail)}.
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}
