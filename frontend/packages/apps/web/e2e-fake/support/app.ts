import { expect, type Locator, type Page } from '@playwright/test';

/**
 * Fake-mode end-to-end support helpers.
 *
 * Each spec drives the real app against the real backend; the only scripted
 * part is the fake the backend spawns as the session's agent — `fake-claude`
 * for a Claude session, `fake-codex` for a Codex one.
 *
 * For `fake-claude`, the backend's wrapper script points
 * `FAKE_CLAUDE_SCENARIO_DIR` at this suite's `scenarios/` directory, and the
 * fake selects `<dir>/<first word of the first prompt>.json` — so the scenario a
 * session follows is chosen by the first prompt each spec sends. Keep the first
 * word of every CLAUDE spec's first prompt equal to a scenario file name. A
 * Codex session's scenario is pinned for the whole run by the server fixture
 * instead (see {@link startNewCodexSession}), so its prompt text is free.
 */

/**
 * Start a new session whose first prompt is `prompt` (its first word selects
 * the fake scenario): enter the new-session flow, choose a directory in the
 * inline Directory tab, and send.
 *
 * By default the picker's initial directory (the browse root, `$HOME`) is
 * chosen — fine for the fake, which ignores its working directory. A real
 * `claude` raises a first-run trust prompt in a directory it has never been
 * trusted in, so the real-claude smoke passes an explicit `workdir` (under
 * the repository, whose trust is already established) and the picker is
 * navigated there segment by segment.
 *
 * Phase B retired the auto-opened modal: the new-session screen shows the
 * 3-tab picker inline (PR / Repository / Directory) and defaults to
 * Repository. The helper switches to the Directory tab and uses its inline
 * Recent + Browse picker, which commits the selection on a row click (no
 * Select button needed).
 *
 * Two entry states exist. On a cold, empty database the app lands directly
 * in the new-session state; with existing sessions, "New" (re)starts the
 * flow. The helper first waits for the app to settle (see
 * {@link waitForSettledApp}) and clicks "New session" only when the
 * new-session screen is not already showing.
 *
 * It returns only once the send has left the new-session screen: the
 * workspace focuses the new session as soon as the send is accepted, so the
 * caller's next step (including another `startNewSession`) starts from that
 * session instead of racing the screen's close.
 */
export async function startNewSession(
  page: Page,
  prompt: string,
  workdir?: string,
): Promise<void> {
  await startSessionOn(page, null, prompt, workdir);
}

/**
 * Like {@link startNewSession}, but on the **Codex** provider: the terminal-less
 * adapter path, whose scripted server is the `fake-codex` binary the backend
 * spawns (its scenario is fixed for the run by the server fixture's wrapper —
 * unlike `fake-claude`, the Codex fake has no prompt-word scenario selection).
 */
export async function startNewCodexSession(
  page: Page,
  prompt: string,
  workdir?: string,
): Promise<void> {
  await startSessionOn(page, 'codex', prompt, workdir);
}

/**
 * The shared body of the two entry points: enter the new-session flow, pick the
 * provider (when one is named — otherwise the form's default, Claude, stands),
 * choose a directory, and send.
 */
async function startSessionOn(
  page: Page,
  provider: 'claude' | 'codex' | null,
  prompt: string,
  workdir?: string,
): Promise<void> {
  const newSessionEmpty = page.getByTestId('new-session-empty');
  await waitForSettledApp(page);
  if (!(await newSessionEmpty.isVisible())) {
    await page.getByRole('button', { name: 'New session', exact: true }).click();
  }

  if (provider !== null) {
    // The provider radio gates on `GET /api/providers`: a provider whose binary
    // the server cannot find renders disabled, so waiting for it to be enabled
    // fails loudly here instead of silently launching the default provider.
    const option = page.getByTestId(`provider-option-${provider}`);
    await expect(option).toBeEnabled();
    await option.check();
  }

  // Switch to the Directory tab so its inline Recent + Browse picker
  // appears (Repository is the default landing tab in Phase B).
  await page.getByTestId('new-session-tab-directory').click();
  await expect(page.getByTestId('workdir-picker')).toBeVisible();
  if (workdir !== undefined) {
    await navigateBrowseTo(page, workdir);
  } else {
    await page.getByTestId('workdir-use-current').click();
  }
  // The inline picker commits on row click — no Select button. Wait for the
  // chip the composer card draws once `newSessionWorkdir` is set, so the
  // following Send finds the committed dir.
  await expect(page.getByTestId('workdir-chip')).toBeVisible();

  await composerInput(page).fill(prompt);
  await page.getByRole('button', { name: 'Send' }).click();
  // The accepted send moves focus to the new session, closing the
  // new-session screen; wait for that so the caller starts from it.
  await expect(newSessionEmpty).toBeHidden();
}

/**
 * Wait for the app's settled state after a load: the navigator lists at least
 * one session, or the new-session screen is showing (the cold-start landing on
 * an empty database). Both can hold at once — for example after a reload onto
 * the new-session screen while sessions exist — so the union is narrowed with
 * `.first()` to keep Playwright's strict mode from rejecting two matches.
 */
export async function waitForSettledApp(page: Page): Promise<void> {
  await expect(
    page
      .getByTestId('session-node')
      .or(page.getByTestId('new-session-empty'))
      .first(),
  ).toBeVisible();
}

/**
 * Navigate the picker's Browse section from its root (`$HOME`) down to
 * `absPath`, clicking one directory segment at a time (the picker has no path
 * input, so `absPath` must be under `$HOME`). The picker hides dot-directories
 * by default, so when any segment starts with a dot the "Show hidden" toggle
 * is turned on once, up front. Entering a directory also makes it the
 * picker's candidate, so the caller only has to confirm afterwards.
 */
async function navigateBrowseTo(page: Page, absPath: string): Promise<void> {
  const home = process.env.HOME;
  if (!home || !absPath.startsWith(`${home}/`)) {
    throw new Error(`workdir must live under $HOME (${home}): ${absPath}`);
  }
  const browse = page.getByTestId('workdir-browse');
  const segments = absPath.slice(home.length + 1).split('/');
  if (segments.some((segment) => segment.startsWith('.'))) {
    // The toggle keeps its state while browsing, so one check covers every
    // dot-segment on the way down.
    await browse.getByRole('checkbox', { name: 'Show hidden' }).check();
  }
  let current = home;
  for (const segment of segments) {
    current = `${current}/${segment}`;
    // Directory rows render their name with a trailing slash ("name/"), which
    // is part of the button's accessible name.
    await browse
      .getByRole('button', { name: `${segment}/`, exact: true })
      .click();
    // The entered directory becomes the listing root; waiting for it keeps
    // the next segment's click off a stale listing.
    await expect(page.getByTestId('workdir-use-current')).toHaveAttribute(
      'title',
      current,
    );
  }
}

/**
 * The terminal on screen. The pane keeps a bound session's xterm attached but
 * hidden while another session is focused — including a session from an
 * earlier spec that the cold load focused first — so a bare `.xterm…` locator
 * can match more than one instance.
 */
export function shownTerminal(page: Page): Locator {
  return page.locator('.xterm').filter({ visible: true });
}

/**
 * The composer's input. A bare `getByRole('textbox')` is not enough: a new
 * session's terminal is open by default on the suite's (large) viewport, and
 * xterm's own helper textarea ("Terminal input") is a textbox too.
 */
export function composerInput(page: Page): Locator {
  return page
    .getByRole('textbox')
    .and(page.locator(':not([aria-label="Terminal input"])'));
}

/** Send a follow-up message into the focused (already started) session. */
export async function sendMessage(page: Page, text: string): Promise<void> {
  await composerInput(page).fill(text);
  await page.getByRole('button', { name: 'Send' }).click();
}

/**
 * The navigator node of the focused session — the one a spec just started.
 *
 * Scope per-session assertions (its `Open` / `Starting` status) to this node
 * rather than counting across the navigator: the suite shares one database and
 * tmux socket, and a server restart re-adopts every Claude pane earlier specs
 * left running, so those sessions read `Open` too.
 */
export function focusedSessionNode(page: Page): Locator {
  return page.locator('[data-testid="session-node"][aria-current="true"]');
}
