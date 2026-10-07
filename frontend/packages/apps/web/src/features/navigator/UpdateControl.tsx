import type { ReactNode } from 'react';
import type { LatestReleaseResponse, NewerRelease } from '@delta/wire-gen';
import { Button, cn } from '@delta/ui-kit';
import {
  useDownloadLatestReleaseMutation,
  useInstallLatestReleaseMutation,
  useRestartLatestReleaseMutation,
} from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';
import { CopyButton } from '../settings/storage/StorageParts';

/** Each state is one phrase that never breaks; the footer row wraps between phrases. */
const PHRASE = 'whitespace-nowrap text-caption';

/** The Update, Install and Restart actions and their retries, styled like the notice link beside them. */
const ACTION = `${PHRASE} text-accent hover:underline`;

/** A state that is not an action. */
const STATUS = `${PHRASE} text-fg-muted`;

/**
 * What the footer shows about the update: `phrase` goes in the footer row
 * beside the newer-release notice (a phrase that never breaks), `problem`
 * under that row, as a block of its own (a failed or impossible install, or
 * a rejected file, whose command and actions would not fit in a phrase).
 * At most one of them is set.
 */
export interface UpdateControlView {
  phrase: ReactNode;
  problem: ReactNode;
}

const NOTHING: UpdateControlView = { phrase: null, problem: null };

const phrase = (node: ReactNode): UpdateControlView => ({
  phrase: node,
  problem: null,
});

const problem = (node: ReactNode): UpdateControlView => ({
  phrase: null,
  problem: node,
});

/**
 * What the footer offers next to the newer-release notice, as the server
 * decides it (`offer`, `installs` and the states in `GET /api/latest-release`):
 *
 * - `update` (a desktop app built by the release workflow, for a release with
 *   an asset it may download): an **Update** control that downloads the
 *   release, then shows the download's state — downloading with its progress,
 *   ready, or failed with a retry and the cause in its tooltip. Where the app
 *   installs updates itself (`installs`, Linux), ready offers **Install**,
 *   which shows installing while the system asks for the password and the
 *   install runs, then **Restart** once installed. A failed install, and an
 *   install Delta cannot run here, offer a retry and show the command that
 *   installs the file from the user's own terminal ({@link ManualInstall}).
 *   A file the update helper rejected (it failed the digest or package check)
 *   was removed by the server: that offers downloading it again and the
 *   release page, never a command that would install it. A dismissed
 *   password dialog leaves the update ready.
 * - `rebuild` (a desktop app built locally): a hint that it is updated by
 *   rebuilding it, since the release would roll its tree back.
 * - `none` (the browser version, a desktop app that cannot download, or a
 *   release with nothing this platform may download): nothing; the notice's
 *   link is all there is.
 *
 * Shows nothing while there is no newer release. The server refuses an
 * action it does not offer whatever this shows; a refusal reads as a failure
 * with the server's message as its cause.
 */
export function useUpdateControl(
  latest: LatestReleaseResponse | null,
): UpdateControlView {
  const client = useApiClient();
  const download = useDownloadLatestReleaseMutation(client);
  const install = useInstallLatestReleaseMutation(client);
  const restart = useRestartLatestReleaseMutation(client);
  if (latest === null || latest.newer === null) {
    return NOTHING;
  }
  const { newer, offer } = latest;
  if (offer === 'rebuild') {
    return phrase(
      <span
        className={STATUS}
        data-testid="update-rebuild-hint"
        title="This app was built locally: update it by rebuilding it (make desktop)"
      >
        rebuild to update
      </span>
    );
  }
  if (offer !== 'update') {
    return NOTHING;
  }

  const installState = latest.installs ? latest.install : null;
  if (install.isPending || installState?.state === 'installing') {
    return phrase(
      <span
        className={STATUS}
        data-testid="update-installing"
        role="status"
        title={`Installing Delta ${newer.version}: enter an administrator's password in the dialog the system shows`}
      >
        Installing…
      </span>,
    );
  }
  if (installState?.state === 'installed') {
    if (restart.isPending || restart.isSuccess) {
      return phrase(
        <span className={STATUS} data-testid="update-restarting" role="status">
          Restarting…
        </span>,
      );
    }
    const cause = restart.error?.message ?? null;
    return phrase(
      <button
        type="button"
        className={ACTION}
        data-testid="update-restart"
        title={
          cause === null
            ? `Delta ${installState.version} is installed: restart the app to run it. As when you quit, Claude Code sessions keep running and Codex sessions end`
            : `Restart failed: ${cause}. Press to try again.`
        }
        onClick={() => restart.mutate()}
      >
        {cause === null ? 'Restart' : 'Restart failed · Retry'}
      </button>,
    );
  }
  if (installState?.state === 'failed') {
    return problem(
      <ProblemBox tone="danger">
        <div className={PROBLEM_HEADING}>
          <Button
            size="sm"
            className="text-left"
            data-testid="update-install-retry"
            title={`Install failed: ${installState.error}. Press to try again.`}
            aria-label={`Retry the install, which failed: ${installState.error}`}
            onClick={() => install.mutate()}
          >
            Install failed · Retry
          </Button>
        </div>
        <ManualInstall command={installState.manual_command} newer={newer} />
      </ProblemBox>,
    );
  }
  if (installState?.state === 'unavailable') {
    return problem(
      <ProblemBox tone="warning">
        <div className={PROBLEM_HEADING}>
          <span
            className="font-medium text-warning"
            data-testid="update-install-unavailable"
            title={installState.error}
          >
            Install it from a terminal
          </span>
          <Button
            size="sm"
            className="text-left"
            data-testid="update-install-retry"
            title={`Delta could not install the update itself: ${installState.error}. Press to try again.`}
            aria-label={`Retry the install, which Delta could not run: ${installState.error}`}
            onClick={() => install.mutate()}
          >
            Install · Retry
          </Button>
        </div>
        <ManualInstall command={installState.manual_command} newer={newer} />
      </ProblemBox>,
    );
  }

  const state = latest.download;
  if (download.isPending || state?.state === 'downloading') {
    const percent =
      state?.state === 'downloading' &&
      state.total_bytes !== null &&
      state.total_bytes > 0
        ? ` ${Math.floor((state.received_bytes * 100) / state.total_bytes)}%`
        : '…';
    return phrase(
      <span
        className={STATUS}
        data-testid="update-downloading"
        role="status"
        title={`Downloading Delta ${newer.version}`}
      >
        {`Downloading${percent}`}
      </span>,
    );
  }
  if (state?.state === 'ready') {
    if (!latest.installs) {
      return phrase(
        <span
          className={STATUS}
          data-testid="update-ready"
          title={`Delta ${state.version} is downloaded and verified: install it from the downloaded file`}
        >
          Update ready
        </span>,
      );
    }
    const cause = install.error?.message ?? null;
    return phrase(
      <>
        <span
          className={STATUS}
          data-testid="update-ready"
          title={`Delta ${state.version} is downloaded and verified`}
        >
          Update ready
        </span>
        <button
          type="button"
          className={ACTION}
          data-testid="update-install"
          title={
            cause === null
              ? `Install Delta ${state.version}: the system asks for an administrator's password`
              : `Install failed: ${cause}. Press to try again.`
          }
          onClick={() => install.mutate()}
        >
          {cause === null ? 'Install' : 'Install failed · Retry'}
        </button>
      </>,
    );
  }

  const cause =
    download.error?.message ?? (state?.state === 'failed' ? state.error : null);
  if (cause === null && installState?.state === 'rejected') {
    return problem(
      <ProblemBox tone="danger">
        <div className={PROBLEM_HEADING}>
          <Button
            size="sm"
            className="text-left"
            data-testid="update-download-again"
            title={`The downloaded file failed verification and was removed: ${installState.error}. Press to download Delta ${newer.version} again.`}
            aria-label={`Download the update again; the downloaded file failed verification: ${installState.error}`}
            onClick={() => download.mutate()}
          >
            Verification failed · Download again
          </Button>
          <ReleasePageLink newer={newer} />
        </div>
      </ProblemBox>,
    );
  }
  if (cause !== null) {
    return phrase(
      <button
        type="button"
        className={ACTION}
        data-testid="update-retry"
        title={`Update failed: ${cause}. Press to try again.`}
        aria-label={`Retry the update, which failed: ${cause}`}
        onClick={() => download.mutate()}
      >
        Update failed · Retry
      </button>,
    );
  }
  return phrase(
    <button
      type="button"
      className={ACTION}
      data-testid="update-button"
      title={`Download Delta ${newer.version} and verify it`}
      onClick={() => download.mutate()}
    >
      Update
    </button>,
  );
}

/**
 * The first line of a problem block: what went wrong, and the action that
 * answers it. The two sit at either end and stack when the column is too
 * narrow for both.
 */
const PROBLEM_HEADING =
  'flex flex-wrap items-center justify-between gap-x-2 gap-y-1';

/**
 * The frame of a problem shown under the footer row: a tinted, bordered box
 * in the tone of the problem — `danger` for a failure, `warning` for an
 * install Delta cannot run but the user can — like the app's other inline
 * errors.
 */
function ProblemBox({
  tone,
  children,
}: {
  tone: 'danger' | 'warning';
  children: ReactNode;
}) {
  return (
    <div
      className={cn(
        'flex flex-col gap-2 rounded border px-2 py-1.5 text-caption text-fg-muted',
        tone === 'danger'
          ? 'border-danger/30 bg-danger/5'
          : 'border-warning/30 bg-warning/5',
      )}
    >
      {children}
    </div>
  );
}

/** The newer release's page, which opens outside the app. */
function ReleasePageLink({ newer }: { newer: NewerRelease }) {
  return (
    <a
      className="whitespace-nowrap text-caption text-accent hover:underline"
      data-testid="update-release-page"
      href={newer.url}
      target="_blank"
      rel="noopener noreferrer"
    >
      release page
    </a>
  );
}

/**
 * The way out when the app cannot install the update: the exact command that
 * installs the verified file from the user's own terminal, and that the app
 * must then be restarted to run it, with a copy button and a link to the
 * release page. The command, a long path, sits in a sunken well where it
 * wraps anywhere rather than widening the column, and stays selectable.
 */
function ManualInstall({
  command,
  newer,
}: {
  command: string;
  newer: NewerRelease;
}) {
  return (
    <div className="flex flex-col gap-1" data-testid="update-manual">
      <span>Run in a terminal, then restart Delta:</span>
      <code
        className="block select-text break-all rounded border border-border-default bg-surface-sunken px-1.5 py-1 font-mono text-code text-fg"
        data-testid="update-manual-command"
      >
        {command}
      </code>
      <div className="flex items-center justify-between gap-2">
        <ReleasePageLink newer={newer} />
        <CopyButton value={command} label="install command" />
      </div>
    </div>
  );
}
