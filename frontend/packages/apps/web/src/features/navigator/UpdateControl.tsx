import type { LatestReleaseResponse } from '@delta/wire-gen';
import { useDownloadLatestReleaseMutation } from '@delta/api-client';
import { useApiClient } from '../../data/apiContext';

/** Each state is one phrase that never breaks; the footer row wraps between phrases. */
const PHRASE = 'whitespace-nowrap text-caption';

/** The Update action and its retry, styled like the notice link beside it. */
const ACTION = `${PHRASE} text-accent hover:underline`;

/**
 * What the footer offers next to the newer-release notice, as the server
 * decides it (`offer` in `GET /api/latest-release`):
 *
 * - `update` (a desktop app built by the release workflow, for a release with
 *   an asset it may download): an **Update** control that downloads the
 *   release, then shows the download's state — downloading with its progress,
 *   ready (verified, waiting for a later step to apply it), or failed with a
 *   retry and the cause in its tooltip.
 * - `rebuild` (a desktop app built locally): a hint that it is updated by
 *   rebuilding it, since the release would roll its tree back.
 * - `none` (the browser version, a desktop app that cannot download, or a
 *   release with nothing this platform may download): nothing; the notice's
 *   link is all there is.
 *
 * Renders nothing while there is no newer release. The server refuses a
 * download it does not offer whatever this shows; a refusal reads as a
 * failure with the server's message as its cause.
 */
export function UpdateControl({ latest }: { latest: LatestReleaseResponse }) {
  const client = useApiClient();
  const download = useDownloadLatestReleaseMutation(client);
  const { newer, offer } = latest;
  if (newer === null) {
    return null;
  }
  if (offer === 'rebuild') {
    return (
      <span
        className={`${PHRASE} text-fg-muted`}
        data-testid="update-rebuild-hint"
        title="This app was built locally: update it by rebuilding it (make desktop)"
      >
        rebuild to update
      </span>
    );
  }
  if (offer !== 'update') {
    return null;
  }

  const state = latest.download;
  if (download.isPending || state?.state === 'downloading') {
    const percent =
      state?.state === 'downloading' &&
      state.total_bytes !== null &&
      state.total_bytes > 0
        ? ` ${Math.floor((state.received_bytes * 100) / state.total_bytes)}%`
        : '…';
    return (
      <span
        className={`${PHRASE} text-fg-muted`}
        data-testid="update-downloading"
        role="status"
        title={`Downloading Delta ${newer.version}`}
      >
        {`Downloading${percent}`}
      </span>
    );
  }
  if (state?.state === 'ready') {
    return (
      <span
        className={`${PHRASE} text-fg-muted`}
        data-testid="update-ready"
        title={`Delta ${state.version} is downloaded and verified, ready to be applied`}
      >
        Update ready
      </span>
    );
  }

  const cause =
    download.error?.message ?? (state?.state === 'failed' ? state.error : null);
  if (cause !== null) {
    return (
      <button
        type="button"
        className={ACTION}
        data-testid="update-retry"
        title={`Update failed: ${cause}. Press to try again.`}
        aria-label={`Retry the update, which failed: ${cause}`}
        onClick={() => download.mutate()}
      >
        Update failed · Retry
      </button>
    );
  }
  return (
    <button
      type="button"
      className={ACTION}
      data-testid="update-button"
      title={`Download Delta ${newer.version} and verify it`}
      onClick={() => download.mutate()}
    >
      Update
    </button>
  );
}
