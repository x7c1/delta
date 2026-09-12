import { fileURLToPath } from 'node:url';
import path from 'node:path';

/**
 * The filesystem paths the fake-mode suite shares across processes: the
 * artifact dir is wiped by `globalSetup.ts` and written by the worker fixture
 * (`server.ts`), so it is spelled exactly once, here.
 */

const HERE = path.dirname(fileURLToPath(import.meta.url));
/** support/ -> e2e-fake/ -> web/ -> apps/ -> packages/ -> frontend/ -> repo root */
export const REPO_ROOT = path.resolve(HERE, '../../../../../..');

/**
 * Where the run's backend diagnostics are preserved.
 *
 * The per-run state (server logs, fake transcripts) lives in a temp dir
 * deleted on teardown, which is useless once CI tears the runner down. The
 * fixture mirrors the diagnostics to this stable, repo-relative path that the
 * CI upload step references (alongside Playwright's own traces/videos/
 * screenshots under `test-results/`).
 *
 * Emptied once per run by `globalSetup.ts`; every server boot then writes
 * under its own `boot-<N>/` subdirectory (see `server.ts`).
 */
export const ARTIFACT_DIR = path.join(
  REPO_ROOT,
  'frontend/packages/apps/web/test-results/e2e-fake',
);
