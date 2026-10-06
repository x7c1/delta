import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { ARTIFACT_DIR, REPO_ROOT } from './paths';

/** Cleans up previous e2e-fake runs; see the script's header for the rule. */
const SWEEP_SCRIPT = path.join(REPO_ROOT, 'scripts/sweep-test-residue.sh');

/**
 * Start every run from a clean slate — once, at the start of the run.
 *
 * First the residue of previous runs: `scripts/sweep-test-residue.sh e2e-fake`
 * kills dead runs' tmux servers and unlinks their sockets, keeps the newest
 * dead run directory as evidence, and removes the older ones. It never
 * touches a run whose owner process is alive (another checkout's run, or a
 * live worker), nor any socket outside the `delta-e2e-fake-*` prefix.
 *
 * Then the artifact dir is emptied.
 *
 * Without the wipe a re-run would append to a stale `server.log` and leave a
 * prior run's `boot-*` directories behind for CI to upload, so an old run's
 * logs could masquerade as this one's. Both steps belong here rather than in
 * `bootServer()` because `bootServer()` runs once per *worker*: Playwright
 * tears a worker down after a failed test and starts a fresh one, and a wipe
 * or sweep on that second boot would delete exactly the evidence covering the
 * failure. `globalSetup` runs once per `playwright test` invocation, in its
 * own process, which is the scope both want.
 *
 * The dir is also Playwright's own `outputDir` (see playwright.fake.config.ts),
 * which Playwright empties at the start of a run — in its "clear output"
 * task, ordered *before* every `globalSetup` (checked in @playwright/test
 * 1.60). So this hook has the last word, and the `boot-<N>/` directories
 * written under it afterwards are never swept away mid-run.
 */
export default function globalSetup(): void {
  execFileSync(SWEEP_SCRIPT, ['e2e-fake'], { stdio: 'inherit' });
  fs.rmSync(ARTIFACT_DIR, { recursive: true, force: true });
  fs.mkdirSync(ARTIFACT_DIR, { recursive: true });
}
