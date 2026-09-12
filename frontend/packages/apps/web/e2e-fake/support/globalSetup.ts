import fs from 'node:fs';
import { ARTIFACT_DIR } from './paths';

/**
 * Start every run from an empty artifact dir — once, at the start of the run.
 *
 * Without this a re-run would append to a stale `server.log` and leave a prior
 * run's `boot-*` directories behind for CI to upload, so an old run's logs
 * could masquerade as this one's. The wipe belongs here rather than in
 * `bootServer()` because `bootServer()` runs once per *worker*: Playwright
 * tears a worker down after a failed test and starts a fresh one, and a wipe
 * on that second boot would delete exactly the logs covering the failure.
 * `globalSetup` runs once per `playwright test` invocation, in its own
 * process, which is the scope the wipe wants.
 *
 * The dir also sits inside Playwright's own `outputDir` (`test-results/`),
 * which Playwright empties at the start of a run — in its "clear output"
 * task, ordered *before* every `globalSetup` (checked in @playwright/test
 * 1.60). So this hook has the last word, and the `boot-<N>/` directories
 * written under it afterwards are never swept away mid-run.
 */
export default function globalSetup(): void {
  fs.rmSync(ARTIFACT_DIR, { recursive: true, force: true });
  fs.mkdirSync(ARTIFACT_DIR, { recursive: true });
}
