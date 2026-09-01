import { spawn, type ChildProcess } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { setTimeout as sleep } from 'node:timers/promises';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

/**
 * The `delta-server` lifecycle, owned by the Playwright worker process.
 *
 * The server-restart semantics under test (a `dispatched` send a dead process
 * left behind, swept back to `queued` + `held_at` at the next boot) cross
 * a server-process death, so a spec must be able to kill the server and bring
 * it back against the *same* database, tmux socket, and scripted-agent
 * wrappers. That is only possible if the test process holds the child-process
 * handle — which `scripts/e2e-fake.sh` used to keep as bash locals and never
 * exposed. This module moves that ownership into the worker: it boots the
 * server, polls `/health`, exposes {@link ServerHandle.restart}, and tears
 * everything down.
 *
 * A single boot implementation lives here (in Node) so the bash entry point
 * and the fixture cannot drift: `scripts/e2e-fake.sh` is now a thin wrapper
 * that only builds the binaries and invokes the suite.
 *
 * ## Per-run isolation (mirrors the retired bash locals)
 *
 * - the SQLite database, spawn workdirs, the scripted-claude and scripted-codex
 *   wrappers, and a pid file live in a fresh temp dir
 *   (`delta-e2e-fake.<rand>/`), removed on teardown;
 * - tmux runs on a unique per-run socket (`delta-e2e-fake-<pid>`), killed on
 *   teardown, so a leftover or parallel run never collides;
 * - the fake's transcripts live under the temp dir too, and are copied into
 *   the CI artifact dir on teardown.
 *
 * ## Log generations
 *
 * One run can now span several server generations (each `restart` spawns a
 * fresh process). Each generation logs to its own file under the artifact
 * dir — `server.log`, `server.2.log`, … — written there directly (not copied
 * on teardown) so a hard crash still leaves every generation's log behind for
 * CI to upload.
 *
 * ## Startup sweep
 *
 * A Node teardown is not guaranteed to run when the Playwright process dies
 * hard (SIGKILL, a Ctrl-C storm), which would leak the per-run tmux server and
 * temp dir. The teardown path is best-effort; the *guarantee* is at startup:
 * before booting, {@link sweepStaleRuns} kills any leftover `delta-e2e-fake-*`
 * tmux server and removes any `delta-e2e-fake.*` temp dir (killing the server
 * process each recorded, via its pid file). Leaks are therefore bounded to one
 * crashed run and cleaned by the next.
 */

/** The default backend port; kept in sync with `playwright.fake.config.ts`. */
const BACKEND_PORT = Number(process.env.E2E_FAKE_BACKEND_PORT ?? 7899);

/**
 * The per-run bearer token every backend generation this fixture spawns is
 * given. It must match the token `playwright.fake.config.ts` injects into the
 * page (its `AUTH_TOKEN`), so the real frontend the fake suite drives presents
 * the token the backend enforces.
 */
const AUTH_TOKEN = 'delta-e2e-fake-auth-token';

/** Recognisable prefixes so the startup sweep can find a previous run's leaks. */
const TMP_PREFIX = 'delta-e2e-fake.';
const SOCKET_PREFIX = 'delta-e2e-fake-';

const HERE = path.dirname(fileURLToPath(import.meta.url));
// support/ -> e2e-fake/ -> web/ -> apps/ -> packages/ -> frontend/ -> repo root
const REPO_ROOT = path.resolve(HERE, '../../../../../..');
const BACKEND_DIR = path.join(REPO_ROOT, 'backend');
const SERVER_BIN = path.join(BACKEND_DIR, 'target/debug/delta-server');
const FAKE_BIN = path.join(BACKEND_DIR, 'target/debug/fake-claude');
const FAKE_CODEX_BIN = path.join(BACKEND_DIR, 'target/debug/fake-codex');
const SCENARIO_DIR = path.join(HERE, '..', 'scenarios');
/**
 * The scripted turn every Codex session in a run plays. `fake-codex` selects its
 * scenario from `FAKE_CODEX_SCENARIO` (one file, read at process start), not from
 * the prompt like `fake-claude` — so the run pins one Codex scenario here and the
 * Codex specs share it.
 *
 * A spec that needs a different Codex turn restarts the server with its own
 * `FAKE_CODEX_SCENARIO` (see {@link ServerHandle.restart} and
 * {@link scenarioPath}); the wrapper below defers to an inherited value, so the
 * override reaches the fake through the server it is spawned from.
 */
const CODEX_SCENARIO = path.join(SCENARIO_DIR, 'codex-parallel-approvals.json');

/**
 * The absolute path of a scenario file by name (without the `.json`), for a spec
 * pinning its own Codex turn through `restart({ FAKE_CODEX_SCENARIO })`.
 */
export function scenarioPath(name: string): string {
  return path.join(SCENARIO_DIR, `${name}.json`);
}

// The per-run state (server.log, transcripts) lives in a temp dir deleted on
// teardown, which is useless once CI tears the runner down. Mirror the
// diagnostics to a stable, repo-relative path the CI upload step references
// (alongside Playwright's own traces/videos/screenshots under test-results/).
const ARTIFACT_DIR = path.join(REPO_ROOT, 'frontend/packages/apps/web/test-results/e2e-fake');

/** Elevated, overridable log level (a caller-provided `RUST_LOG` wins). */
const SERVER_RUST_LOG = process.env.RUST_LOG ?? 'delta_usecase=debug,info';

/** The handle a spec uses to drive the server it is running against. */
export interface ServerHandle {
  /** The backend port the server (and the Vite proxy) is bound to. */
  readonly port: number;
  /**
   * SIGKILL the current server child (a hard death — the production incident
   * was never a graceful shutdown) and relaunch against the SAME database,
   * tmux socket, and scripted-agent wrappers, polling `/health` until the new
   * generation is ready. The relaunched generation logs to the next log file.
   *
   * `env` REPLACES the extra environment applied on top of the per-run
   * defaults, so `restart()` with no argument always returns the server to
   * exactly the suite's shared configuration. Pass overrides only for a
   * server-wide setting a single spec must change (e.g. shrinking a watchdog
   * deadline the whole process shares), and restore them in an `afterEach` so
   * a failure cannot leak the setting into the specs that follow.
   */
  restart(env?: Record<string, string>): Promise<void>;
  /** Kill the server and tmux, copy transcripts out, and remove the temp dir. */
  teardown(): Promise<void>;
}

/** Whether `pid` is still alive. */
function isAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/** Best-effort check that `pid` is a delta-server (Linux `/proc` only). */
function looksLikeServer(pid: number): boolean {
  try {
    const cmdline = fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8');
    return cmdline.includes('delta-server');
  } catch {
    // No /proc (macOS) or the process is gone: fall back to killing it — the
    // pid was recorded by a previous e2e-fake run whose temp dir we are about
    // to delete, so a surviving process of that run is stale regardless.
    return true;
  }
}

/** The directory tmux keeps its named sockets in, for the current user. */
function tmuxSocketDir(): string {
  const base = process.env.TMUX_TMPDIR ?? '/tmp';
  const uid = typeof process.getuid === 'function' ? process.getuid() : 0;
  return path.join(base, `tmux-${uid}`);
}

/**
 * Kill any leftover tmux server and temp dir from a previously crashed run.
 * Best-effort throughout: a failure to clean one leak must not block this run.
 */
function sweepStaleRuns(): void {
  // Stale tmux servers: one named socket file per leaked run.
  const socketDir = tmuxSocketDir();
  let sockets: string[] = [];
  try {
    sockets = fs.readdirSync(socketDir);
  } catch {
    sockets = [];
  }
  for (const name of sockets) {
    if (!name.startsWith(SOCKET_PREFIX)) {
      continue;
    }
    const child = spawn('tmux', ['-L', name, 'kill-server'], {
      stdio: 'ignore',
    });
    child.on('error', () => {
      /* tmux missing or already dead — nothing to clean. */
    });
  }

  // Stale temp dirs: kill each recorded server pid, then remove the dir.
  const tmpBase = os.tmpdir();
  let entries: string[] = [];
  try {
    entries = fs.readdirSync(tmpBase);
  } catch {
    entries = [];
  }
  for (const name of entries) {
    if (!name.startsWith(TMP_PREFIX)) {
      continue;
    }
    const dir = path.join(tmpBase, name);
    try {
      const pids = fs
        .readFileSync(path.join(dir, 'server.pids'), 'utf8')
        .split('\n')
        .map((line) => Number(line.trim()))
        .filter((pid) => Number.isInteger(pid) && pid > 0);
      for (const pid of pids) {
        if (isAlive(pid) && looksLikeServer(pid)) {
          try {
            process.kill(pid, 'SIGKILL');
          } catch {
            /* already gone */
          }
        }
      }
    } catch {
      /* no pid file recorded — nothing to kill for this leak. */
    }
    fs.rmSync(dir, { recursive: true, force: true });
  }

  // Stale tmux config files: the server writes /tmp/delta-tmux-<socket>.conf
  // for every socket it opens and never deletes it, so even clean runs leave
  // one behind. Prefix-matched to this suite's sockets, so a dev socket's
  // conf is never touched.
  for (const name of entries) {
    if (!name.startsWith(`delta-tmux-${SOCKET_PREFIX}`) || !name.endsWith('.conf')) {
      continue;
    }
    fs.rmSync(path.join(tmpBase, name), { force: true });
  }
}

/** Poll `/health` until the server answers ok, or throw with the log tail. */
async function waitHealthy(
  port: number,
  isChildAlive: () => boolean,
  logPath: string,
): Promise<void> {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (!isChildAlive()) {
      throw new Error(
        `delta-server exited during startup. Log tail:\n${logTail(logPath)}`,
      );
    }
    try {
      const res = await fetch(`http://127.0.0.1:${port}/health`);
      if (res.ok) {
        return;
      }
    } catch {
      /* not up yet */
    }
    await sleep(100);
  }
  throw new Error(
    `delta-server did not become healthy on port ${port}. Log tail:\n${logTail(logPath)}`,
  );
}

/** The last few KB of a log file, for a startup-failure message. */
function logTail(logPath: string): string {
  try {
    const content = fs.readFileSync(logPath, 'utf8');
    return content.slice(-4000);
  } catch {
    return '(no log captured)';
  }
}

/**
 * Boot a fresh server for this worker: sweep prior leaks, lay down the per-run
 * temp dir and the two scripted-agent wrappers (claude and codex), spawn
 * generation 1, and wait for `/health`. Returns the handle the fixture hands to
 * specs.
 */
export async function bootServer(): Promise<ServerHandle> {
  sweepStaleRuns();

  const runDir = fs.mkdtempSync(path.join(os.tmpdir(), TMP_PREFIX));
  const dbPath = path.join(runDir, 'delta.db');
  const workdir = path.join(runDir, 'workdir');
  const transcripts = path.join(runDir, 'transcripts');
  const pidFile = path.join(runDir, 'server.pids');
  const tmuxSocket = `${SOCKET_PREFIX}${process.pid}`;
  fs.mkdirSync(workdir, { recursive: true });
  fs.mkdirSync(transcripts, { recursive: true });
  // Start each run from a clean artifact dir so a previous run's logs never
  // masquerade as this one's: generation logs are opened in append mode and
  // named per generation (server.log, server.2.log, …), so without this a
  // re-run would append to a stale server.log and leave orphaned server.N.log
  // / transcripts from a prior multi-generation run for CI to upload.
  fs.rmSync(ARTIFACT_DIR, { recursive: true, force: true });
  fs.mkdirSync(ARTIFACT_DIR, { recursive: true });

  if (
    !fs.existsSync(SERVER_BIN) ||
    !fs.existsSync(FAKE_BIN) ||
    !fs.existsSync(FAKE_CODEX_BIN)
  ) {
    throw new Error(
      `Missing built binaries (${SERVER_BIN}, ${FAKE_BIN}, ${FAKE_CODEX_BIN}). ` +
        `Run \`make e2e-fake\` (it builds them) rather than invoking Playwright directly.`,
    );
  }

  // The server's spawn command line is fixed, so per-run configuration reaches
  // the fake through this wrapper. It pins the transcript directory always, and
  // the scenario directory only on a FRESH spawn: a `claude --resume <id>`
  // carries no positional prompt, and the fake selects its scenario from the
  // first prompt word — so on resume we drop `FAKE_CLAUDE_SCENARIO_DIR` and let
  // the fake fall back to its built-in echo loop (await_prompt → reply → stop),
  // which is exactly what a reopened session's released send needs to complete.
  const wrapper = path.join(runDir, 'claude-bin.sh');
  fs.writeFileSync(
    wrapper,
    `#!/bin/sh\n` +
      `export FAKE_CLAUDE_TRANSCRIPT_DIR='${transcripts}'\n` +
      `case " $* " in\n` +
      `  *' --resume '*) ;;\n` +
      `  *) export FAKE_CLAUDE_SCENARIO_DIR='${SCENARIO_DIR}' ;;\n` +
      `esac\n` +
      `exec '${FAKE_BIN}' "$@"\n`,
    { mode: 0o755 },
  );

  // The Codex counterpart: the adapter spawns this as the `codex` binary (the
  // fake IS the app-server, so the `app-server` argument it is handed is
  // ignored). The wrapper is what hands the fake its scenario, and its mere
  // existence is also what makes the provider selector's Codex option
  // selectable — `GET /api/providers` probes exactly this path.
  const codexWrapper = path.join(runDir, 'codex-bin.sh');
  fs.writeFileSync(
    codexWrapper,
    `#!/bin/sh\n` +
      // The run's shared scenario, unless the server that spawned this wrapper
      // was started with one of its own (a spec pinning its own Codex turn).
      `export FAKE_CODEX_SCENARIO="\${FAKE_CODEX_SCENARIO:-${CODEX_SCENARIO}}"\n` +
      `exec '${FAKE_CODEX_BIN}' "$@"\n`,
    { mode: 0o755 },
  );

  let generation = 0;
  let child: ChildProcess | null = null;
  /** Per-spec environment layered over the defaults; see `restart`. */
  let envOverrides: Record<string, string> = {};

  const spawnGeneration = async (): Promise<void> => {
    generation += 1;
    const logPath = path.join(
      ARTIFACT_DIR,
      generation === 1 ? 'server.log' : `server.${generation}.log`,
    );
    const logFd = fs.openSync(logPath, 'a');
    try {
      const proc = spawn(SERVER_BIN, [], {
        stdio: ['ignore', logFd, logFd],
        env: {
          ...process.env,
          RUST_LOG: SERVER_RUST_LOG,
          DELTA_PORT: String(BACKEND_PORT),
          DELTA_AUTH_TOKEN: AUTH_TOKEN,
          DELTA_DB_PATH: dbPath,
          DELTA_SESSION_WORKDIR: workdir,
          DELTA_TMUX_SOCKET: tmuxSocket,
          DELTA_CLAUDE_BIN: wrapper,
          DELTA_CODEX_BIN: codexWrapper,
          DELTA_LAUNCH_DEADLINE_MS: '3000',
          DELTA_PERMISSION_DECISION_TIMEOUT_MS: '3000',
          // The echo watchdog stays near its production generosity for the
          // shared suite: several specs deliberately hold a send `dispatched`
          // with no echo (cancel, restart) and must not have it retried or
          // parked out from under them. The spec that exercises the watchdog
          // shrinks this for its own server generation via `restart`.
          DELTA_ECHO_DEADLINE_MS: '60000',
          ...envOverrides,
        },
      });
      child = proc;
      let exited = false;
      proc.on('exit', () => {
        exited = true;
      });
      if (proc.pid !== undefined) {
        fs.appendFileSync(pidFile, `${proc.pid}\n`);
      }
      await waitHealthy(BACKEND_PORT, () => !exited, logPath);
    } finally {
      fs.closeSync(logFd);
    }
  };

  await spawnGeneration();

  const killChild = async (): Promise<void> => {
    const proc = child;
    if (!proc || proc.exitCode !== null || proc.pid === undefined) {
      return;
    }
    const done = new Promise<void>((resolve) => proc.once('exit', () => resolve()));
    try {
      proc.kill('SIGKILL');
    } catch {
      /* already gone */
    }
    await done;
  };

  return {
    port: BACKEND_PORT,
    async restart(env: Record<string, string> = {}): Promise<void> {
      envOverrides = env;
      await killChild();
      await spawnGeneration();
    },
    async teardown(): Promise<void> {
      await killChild();
      // Copy the fake transcripts out of the soon-to-be-deleted temp dir into
      // the stable artifact dir CI uploads (best-effort — a run that never
      // spawned claude leaves none).
      try {
        fs.cpSync(transcripts, path.join(ARTIFACT_DIR, 'transcripts'), {
          recursive: true,
        });
      } catch {
        /* nothing to preserve */
      }
      // Kill the whole per-run tmux server: every pane this run spawned dies
      // with it; other sockets (a developer's `delta`, another run) are
      // untouched.
      await new Promise<void>((resolve) => {
        const tmux = spawn('tmux', ['-L', tmuxSocket, 'kill-server'], {
          stdio: 'ignore',
        });
        tmux.on('error', () => resolve());
        tmux.on('exit', () => resolve());
      });
      fs.rmSync(runDir, { recursive: true, force: true });
    },
  };
}
