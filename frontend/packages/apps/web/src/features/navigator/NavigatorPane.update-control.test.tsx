/**
 * The footer's update control: downloading, installing and restarting into
 * a newer release.
 *
 * Shared fixtures live in NavigatorPane.testkit.tsx.
 */
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { MOCK_NEWER_RELEASE } from '@delta/api-mocks';
import type {
  InstallUnavailable,
  LatestReleaseResponse,
  UpdateDownload,
  UpdateInstall,
  UpdateOffer,
} from '@delta/wire-gen';
import { useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';
import { renderPane, server } from './NavigatorPane.testkit';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('NavigatorPane update control', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      rateLimits: {},
    });
    useNavStore.setState({
      focusedSessionId: null,
      activeThreadId: null,
      settingsOpen: false,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  /**
   * Answer `GET /api/latest-release` with the newer release, `offer`,
   * `download`, and — for an app that installs updates itself (`installs`) —
   * `install`, or — for one that found at startup it cannot —
   * `installUnavailable`.
   */
  function answerLatest(
    offer: UpdateOffer,
    download: () => UpdateDownload | null = () => null,
    {
      installs = false,
      install = () => null,
      installUnavailable,
    }: {
      installs?: boolean;
      install?: () => UpdateInstall | null;
      installUnavailable?: InstallUnavailable;
    } = {},
  ) {
    server.use(
      http.get('*/api/latest-release', () =>
        HttpResponse.json({
          newer: MOCK_NEWER_RELEASE,
          offer,
          download: download(),
          installs,
          ...(installUnavailable === undefined
            ? {}
            : { install_unavailable: installUnavailable }),
          install: install(),
        } satisfies LatestReleaseResponse),
      ),
    );
  }

  async function noticeShown() {
    await screen.findByTestId('newer-release');
  }

  const CONTROLS = [
    'update-button',
    'update-rebuild-hint',
    'update-downloading',
    'update-ready',
    'update-retry',
    'update-install',
    'update-installing',
    'update-restart',
    'update-restarting',
    'update-install-retry',
    'update-install-unavailable',
    'update-install-reason',
    'update-manual',
    'update-download-again',
    'update-unavailable',
  ];

  function shownControls() {
    return CONTROLS.filter((id) => screen.queryByTestId(id) !== null);
  }

  it('offers Update only to a desktop release build', async () => {
    answerLatest('update');
    renderPane();
    await noticeShown();
    expect(await screen.findByTestId('update-button')).toHaveTextContent(
      'Update',
    );
    expect(shownControls()).toEqual(['update-button']);
  });

  it('shows the rebuild hint, not Update, to a desktop local build', async () => {
    answerLatest('rebuild');
    renderPane();
    await noticeShown();
    expect(await screen.findByTestId('update-rebuild-hint')).toHaveAttribute(
      'title',
      expect.stringContaining('make desktop'),
    );
    expect(shownControls()).toEqual(['update-rebuild-hint']);
  });

  it('shows neither to the browser version', async () => {
    answerLatest('none');
    renderPane();
    await noticeShown();
    expect(shownControls()).toEqual([]);
  });

  it('shows nothing beside the version when there is no newer release', async () => {
    let asked = false;
    server.use(
      http.get('*/api/latest-release', () => {
        asked = true;
        return HttpResponse.json({
          newer: null,
          offer: 'update',
          download: null,
          installs: false,
          install: null,
        } satisfies LatestReleaseResponse);
      }),
    );
    renderPane();
    await waitFor(() => expect(asked).toBe(true));
    expect(shownControls()).toEqual([]);
  });

  it('starts the download on Update and follows it to ready', async () => {
    let download: UpdateDownload | null = null;
    let posts = 0;
    answerLatest('update', () => download);
    server.use(
      http.post('*/api/latest-release/download', () => {
        posts += 1;
        download = {
          state: 'downloading',
          version: MOCK_NEWER_RELEASE.version,
          received_bytes: 512,
          total_bytes: 2048,
        };
        return HttpResponse.json(download satisfies UpdateDownload, {
          status: 202,
        });
      }),
    );
    renderPane();

    fireEvent.click(await screen.findByTestId('update-button'));

    expect(await screen.findByTestId('update-downloading')).toHaveTextContent(
      'Downloading 25%',
    );
    expect(posts).toBe(1);

    download = { state: 'ready', version: MOCK_NEWER_RELEASE.version };
    expect(
      await screen.findByTestId('update-ready', {}, { timeout: 3000 }),
    ).toHaveTextContent('Update ready');
    expect(shownControls()).toEqual(['update-ready']);
  });

  it('renders a running download with its progress', async () => {
    answerLatest('update', () => ({
      state: 'downloading',
      version: MOCK_NEWER_RELEASE.version,
      received_bytes: 0,
      total_bytes: null,
    }));
    renderPane();
    expect(await screen.findByTestId('update-downloading')).toHaveTextContent(
      'Downloading…',
    );
    expect(shownControls()).toEqual(['update-downloading']);
  });

  it('renders a failed download with its cause and retries it', async () => {
    let download: UpdateDownload | null = {
      state: 'failed',
      version: MOCK_NEWER_RELEASE.version,
      error: 'the download answered with HTTP status 404',
    };
    let posts = 0;
    answerLatest('update', () => download);
    server.use(
      http.post('*/api/latest-release/download', () => {
        posts += 1;
        download = {
          state: 'downloading',
          version: MOCK_NEWER_RELEASE.version,
          received_bytes: 0,
          total_bytes: null,
        };
        return HttpResponse.json(download satisfies UpdateDownload, {
          status: 202,
        });
      }),
    );
    renderPane();

    const retry = await screen.findByTestId('update-retry');
    expect(retry).toHaveTextContent('Update failed · Retry');
    expect(retry).toHaveAttribute(
      'title',
      expect.stringContaining('the download answered with HTTP status 404'),
    );

    fireEvent.click(retry);

    expect(await screen.findByTestId('update-downloading')).toBeInTheDocument();
    expect(posts).toBe(1);
  });

  it('shows a refusal as a failure with the server message', async () => {
    answerLatest('update');
    server.use(
      http.post('*/api/latest-release/download', () =>
        HttpResponse.json(
          {
            error: 'release v0.0.1 has no asset delta-desktop_0.0.1_amd64.deb',
            code: 'update_unsupported',
          },
          { status: 409 },
        ),
      ),
    );
    renderPane();

    fireEvent.click(await screen.findByTestId('update-button'));

    expect(await screen.findByTestId('update-retry')).toHaveAttribute(
      'title',
      expect.stringContaining('has no asset delta-desktop_0.0.1_amd64.deb'),
    );
  });

  const READY: UpdateDownload = {
    state: 'ready',
    version: MOCK_NEWER_RELEASE.version,
  };
  const MANUAL_COMMAND =
    'sudo apt install /home/dev/.local/share/io.github.x7c1.delta/updates/delta-desktop_0.6.0_amd64.deb';
  const DISK_IMAGE =
    '/Users/dev/Library/Application Support/io.github.x7c1.delta/updates/Delta_0.6.0_aarch64.dmg';
  const TRANSLOCATED =
    'Delta cannot install the update itself: macOS runs Delta from a read-only copy (App Translocation), since it was opened from its disk image or the folder it was downloaded to: move Delta to /Applications';

  it('offers no Install but the reason and the disk image once ready where the app found at startup it cannot install', async () => {
    answerLatest('update', () => READY, {
      installUnavailable: {
        error: TRANSLOCATED,
        manual: { kind: 'disk_image', path: DISK_IMAGE },
      },
    });
    renderPane();

    expect(
      await screen.findByTestId('update-install-unavailable'),
    ).toHaveTextContent('Install it from the disk image');
    expect(screen.getByTestId('update-install-reason')).toHaveTextContent(
      'move Delta to /Applications',
    );
    expect(shownControls()).toEqual([
      'update-install-unavailable',
      'update-install-reason',
      'update-manual',
    ]);
    expect(screen.getByTestId('update-manual')).toHaveTextContent(
      'Open the downloaded disk image, quit Delta, drag Delta to Applications, then start Delta again:',
    );
    expect(screen.getByTestId('update-manual-image')).toHaveTextContent(
      DISK_IMAGE,
    );
  });

  it('offers no Install where the app does not install updates itself', async () => {
    answerLatest('update', () => READY);
    renderPane();
    expect(await screen.findByTestId('update-ready')).toHaveTextContent(
      'Update ready',
    );
    expect(shownControls()).toEqual(['update-ready']);
  });

  it('offers Install once the download is ready and follows it to Restart', async () => {
    let install: UpdateInstall | null = null;
    let posts = 0;
    answerLatest('update', () => READY, {
      installs: true,
      install: () => install,
    });
    server.use(
      http.post('*/api/latest-release/install', () => {
        posts += 1;
        install = { state: 'installing', version: MOCK_NEWER_RELEASE.version };
        return HttpResponse.json(install satisfies UpdateInstall, {
          status: 202,
        });
      }),
    );
    renderPane();

    expect(await screen.findByTestId('update-install')).toHaveTextContent(
      'Install',
    );
    expect(shownControls()).toEqual(['update-ready', 'update-install']);
    fireEvent.click(screen.getByTestId('update-install'));

    expect(await screen.findByTestId('update-installing')).toHaveTextContent(
      'Installing…',
    );
    expect(shownControls()).toEqual(['update-installing']);
    expect(posts).toBe(1);

    install = { state: 'installed', version: MOCK_NEWER_RELEASE.version };
    expect(
      await screen.findByTestId('update-restart', {}, { timeout: 3000 }),
    ).toHaveTextContent('Restart');
    expect(shownControls()).toEqual(['update-restart']);
  });

  it('restarts the app on Restart', async () => {
    let restarts = 0;
    answerLatest('update', () => READY, {
      installs: true,
      install: () => ({
        state: 'installed',
        version: MOCK_NEWER_RELEASE.version,
      }),
    });
    server.use(
      http.post('*/api/latest-release/restart', () => {
        restarts += 1;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    renderPane();

    fireEvent.click(await screen.findByTestId('update-restart'));

    expect(await screen.findByTestId('update-restarting')).toHaveTextContent(
      'Restarting…',
    );
    expect(restarts).toBe(1);
  });

  it('renders a failed install with its cause, the manual command and a retry', async () => {
    let install: UpdateInstall | null = {
      state: 'failed',
      version: MOCK_NEWER_RELEASE.version,
      error: 'apt-get could not install the update: E: Unmet dependencies',
      manual: { kind: 'command', command: MANUAL_COMMAND },
    };
    let posts = 0;
    answerLatest('update', () => READY, {
      installs: true,
      install: () => install,
    });
    server.use(
      http.post('*/api/latest-release/install', () => {
        posts += 1;
        install = { state: 'installing', version: MOCK_NEWER_RELEASE.version };
        return HttpResponse.json(install satisfies UpdateInstall, {
          status: 202,
        });
      }),
    );
    renderPane();

    const retry = await screen.findByTestId('update-install-retry');
    expect(retry).toHaveTextContent('Install failed · Retry');
    expect(retry).toHaveAttribute(
      'title',
      expect.stringContaining('E: Unmet dependencies'),
    );
    expect(shownControls()).toEqual(['update-install-retry', 'update-manual']);
    expect(screen.getByTestId('update-manual-command')).toHaveTextContent(
      MANUAL_COMMAND,
    );
    expect(screen.getByTestId('update-release-page')).toHaveAttribute(
      'href',
      MOCK_NEWER_RELEASE.url,
    );

    fireEvent.click(retry);
    expect(await screen.findByTestId('update-installing')).toBeInTheDocument();
    expect(posts).toBe(1);
  });

  it('shows the manual command with a copy button when Delta cannot install', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
    answerLatest('update', () => READY, {
      installs: true,
      install: () => ({
        state: 'unavailable',
        version: MOCK_NEWER_RELEASE.version,
        error:
          'Delta cannot install the update itself: not authorized, or no polkit authentication agent is running',
        manual: { kind: 'command', command: MANUAL_COMMAND },
      }),
    });
    renderPane();

    expect(
      await screen.findByTestId('update-install-unavailable'),
    ).toHaveAttribute('title', expect.stringContaining('no polkit'));
    expect(screen.getByTestId('update-install-unavailable')).toHaveTextContent(
      'Install it from a terminal',
    );
    expect(screen.getByTestId('update-manual')).toHaveTextContent(
      'Run in a terminal, then restart Delta:',
    );
    expect(shownControls()).toEqual([
      'update-install-retry',
      'update-install-unavailable',
      'update-manual',
    ]);
    expect(screen.getByTestId('update-manual-command')).toHaveTextContent(
      MANUAL_COMMAND,
    );

    const copy = screen.getByRole('button', { name: 'Copy install command' });
    fireEvent.click(copy);
    expect(await within(copy).findByText('Copied')).toBeInTheDocument();
    expect(writeText).toHaveBeenCalledWith(MANUAL_COMMAND);
  });

  it('shows the disk image to open and the reason, not a command or a retry, when Delta cannot install on macOS', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
    answerLatest('update', () => READY, {
      installs: true,
      install: () => ({
        state: 'unavailable',
        version: MOCK_NEWER_RELEASE.version,
        error: TRANSLOCATED,
        manual: { kind: 'disk_image', path: DISK_IMAGE },
      }),
    });
    renderPane();

    const unavailable = await screen.findByTestId('update-install-unavailable');
    expect(unavailable).toHaveTextContent('Install it from the disk image');
    expect(screen.getByTestId('update-install-reason')).toHaveTextContent(
      TRANSLOCATED,
    );
    expect(shownControls()).toEqual([
      'update-install-unavailable',
      'update-install-reason',
      'update-manual',
    ]);
    expect(screen.getByTestId('update-manual')).toHaveTextContent(
      'Open the downloaded disk image, quit Delta, drag Delta to Applications, then start Delta again:',
    );
    expect(screen.getByTestId('update-manual-image')).toHaveTextContent(
      DISK_IMAGE,
    );
    expect(screen.queryByTestId('update-manual-command')).toBeNull();
    expect(screen.getByTestId('update-manual')).not.toHaveTextContent(
      'terminal',
    );

    const copy = screen.getByRole('button', { name: 'Copy disk image path' });
    fireEvent.click(copy);
    expect(await within(copy).findByText('Copied')).toBeInTheDocument();
    expect(writeText).toHaveBeenCalledWith(DISK_IMAGE);
  });

  it('shows the disk image to open beside a failed install on macOS', async () => {
    answerLatest('update', () => READY, {
      installs: true,
      install: () => ({
        state: 'failed',
        version: MOCK_NEWER_RELEASE.version,
        error: '/usr/bin/hdiutil failed: hdiutil: attach failed',
        manual: { kind: 'disk_image', path: DISK_IMAGE },
      }),
    });
    renderPane();

    const retry = await screen.findByTestId('update-install-retry');
    expect(retry).toHaveTextContent('Install failed · Retry');
    expect(retry).toHaveAttribute(
      'title',
      expect.stringContaining('hdiutil: attach failed'),
    );
    expect(shownControls()).toEqual(['update-install-retry', 'update-manual']);
    expect(screen.getByTestId('update-manual-image')).toHaveTextContent(
      DISK_IMAGE,
    );
    expect(screen.queryByTestId('update-manual-command')).toBeNull();
  });

  it('offers a retry beside the manual command when Delta cannot install', async () => {
    let install: UpdateInstall | null = {
      state: 'unavailable',
      version: MOCK_NEWER_RELEASE.version,
      error:
        'Delta cannot install the update itself: not authorized, or no polkit authentication agent is running',
      manual: { kind: 'command', command: MANUAL_COMMAND },
    };
    let posts = 0;
    answerLatest('update', () => READY, {
      installs: true,
      install: () => install,
    });
    server.use(
      http.post('*/api/latest-release/install', () => {
        posts += 1;
        install = { state: 'installing', version: MOCK_NEWER_RELEASE.version };
        return HttpResponse.json(install satisfies UpdateInstall, {
          status: 202,
        });
      }),
    );
    renderPane();

    const retry = await screen.findByTestId('update-install-retry');
    expect(retry).toHaveTextContent('Install · Retry');
    expect(retry).toHaveAttribute('title', expect.stringContaining('no polkit'));

    fireEvent.click(retry);
    expect(await screen.findByTestId('update-installing')).toBeInTheDocument();
    expect(posts).toBe(1);
  });

  it('offers downloading again, not a manual command, for a rejected file', async () => {
    const reason = "the update file's sha256 is 00, not release v0.6.0's ff";
    let download: UpdateDownload | null = null;
    let install: UpdateInstall | null = {
      state: 'rejected',
      version: MOCK_NEWER_RELEASE.version,
      error: reason,
    };
    let posts = 0;
    answerLatest('update', () => download, {
      installs: true,
      install: () => install,
    });
    server.use(
      http.post('*/api/latest-release/download', () => {
        posts += 1;
        install = null;
        download = {
          state: 'downloading',
          version: MOCK_NEWER_RELEASE.version,
          received_bytes: 0,
          total_bytes: null,
        };
        return HttpResponse.json(download satisfies UpdateDownload, {
          status: 202,
        });
      }),
    );
    renderPane();

    const again = await screen.findByTestId('update-download-again');
    expect(again).toHaveTextContent('Verification failed · Download again');
    expect(again).toHaveAttribute('title', expect.stringContaining(reason));
    expect(shownControls()).toEqual(['update-download-again']);
    expect(screen.getByTestId('update-release-page')).toHaveAttribute(
      'href',
      MOCK_NEWER_RELEASE.url,
    );
    expect(screen.queryByTestId('update-manual-command')).toBeNull();

    fireEvent.click(again);
    expect(await screen.findByTestId('update-downloading')).toBeInTheDocument();
    expect(posts).toBe(1);
  });

  it('shows an unverifiable release as unavailable, with the way by hand and no retry', async () => {
    const reason =
      "the release's signature does not verify the downloaded file: the file does not match its signature";
    answerLatest(
      'update',
      () => ({
        state: 'unavailable',
        version: MOCK_NEWER_RELEASE.version,
        error: reason,
      }),
      { installs: true },
    );
    renderPane();

    const heading = await screen.findByTestId('update-unavailable');
    expect(heading).toHaveTextContent('Update it by hand');
    expect(heading).toHaveAttribute('title', reason);
    expect(screen.getByTestId('update-unavailable-reason')).toHaveTextContent(
      reason,
    );
    expect(screen.getByTestId('update-unavailable-steps')).toHaveTextContent(
      'check its signature',
    );
    expect(screen.getByTestId('update-release-page')).toHaveAttribute(
      'href',
      MOCK_NEWER_RELEASE.url,
    );
    // Nothing to install, retry or download again.
    expect(shownControls()).toEqual(['update-unavailable']);
    expect(screen.queryByTestId('update-manual-command')).toBeNull();
  });

  it('shows an install refusal as a failure with the server message', async () => {
    answerLatest('update', () => READY, { installs: true });
    server.use(
      http.post('*/api/latest-release/install', () =>
        HttpResponse.json(
          {
            error: 'no verified download of the newer release is ready to install',
            code: 'update_not_ready',
          },
          { status: 409 },
        ),
      ),
    );
    renderPane();

    fireEvent.click(await screen.findByTestId('update-install'));

    await waitFor(() =>
      expect(screen.getByTestId('update-install')).toHaveTextContent(
        'Install failed · Retry',
      ),
    );
    expect(screen.getByTestId('update-install')).toHaveAttribute(
      'title',
      expect.stringContaining('no verified download'),
    );
  });
});
