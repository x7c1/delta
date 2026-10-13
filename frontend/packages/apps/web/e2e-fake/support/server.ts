import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { setTimeout as sleep } from 'node:timers/promises';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { AUTH_TOKEN, BACKEND_PORT, DEV_SERVER_PORT } from './endpoints';
import { ARTIFACT_DIR, REPO_ROOT } from './paths';

/**
 * The lifecycle of the suite's two servers — `delta-server` and the Vite dev
 * server the page loads — owned by the Playwright worker process.
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
 * - the server's data directory is a fresh temp dir (`delta-e2e-fake.<rand>/`,
 *   handed over as `DELTA_DATA_DIR`): the SQLite database, the session
 *   settings, the tmux configuration and the spawn workdirs land in it,
 *   beside the scripted-claude and scripted-codex wrappers, the server pid
 *   file and an `owner.pid` naming this worker;
 * - tmux runs on a unique per-run socket (`delta-e2e-fake-<pid>`), killed on
 *   teardown, so a leftover or parallel run never collides;
 * - the fake's transcripts live under the temp dir too, and are copied into
 *   this boot's directory under the CI artifact dir on teardown.
 *
 * ## Log generations, one directory per boot
 *
 * One run can now span several server generations (each `restart` spawns a
 * fresh process). Each generation logs to its own file — `server.log`,
 * `server.2.log`, …, and likewise `vite.log`, `vite.2.log`, … for the dev
 * server — written there directly (not copied on teardown) so a
 * hard crash still leaves every generation's log behind for CI to upload.
 *
 * Those files, and the transcripts copied out on teardown, land under a
 * directory private to this boot: `<artifact dir>/boot-<N>/`. A run can boot
 * the server more than once — Playwright tears a worker down after a failed
 * test and runs the fixture again in a fresh one — and the reboot must not
 * overwrite the evidence of the failure that caused it. So the reboot writes
 * `boot-2/` next to `boot-1/`, and the artifact dir itself is emptied once per
 * run, by `globalSetup.ts`, never here.
 *
 * ## The dev server is this worker's too, and never adopted
 *
 * The Vite dev server is started here rather than by Playwright's `webServer`,
 * for two reasons. A `webServer` outlives every worker, so a spec that took it
 * down left every later spec failing with a refused connection, and nothing
 * could bring it back; a worker-owned one is restarted like the backend. And
 * `webServer` adopted whatever already answered on its port — a killed run's
 * orphaned Vite, or another checkout's run in progress — a process this run
 * does not own and that can vanish under it mid-suite. Here neither server is
 * ever adopted: each start first waits for its port to be released, and fails
 * naming the process that still holds it.
 *
 * ## Health is checked around every spec
 *
 * {@link ServerHandle.findDown} and {@link ServerHandle.restoreDown} back a
 * per-spec fixture (`fixtures.ts`) that restores a server found down before a
 * spec and fails the spec that leaves one down, so a dead server is reported
 * once, against the spec that caused it, instead of as a connection error in
 * every spec after it.
 *
 * ## Teardown keeps the run directory; the next run sweeps it
 *
 * Teardown kills both servers and the per-run tmux server but leaves the run
 * directory in place: its database is the evidence a failure investigation
 * reads. Cleanup happens at the start of the next run instead, through the
 * sweep in `globalSetup.ts` (once per invocation, never per worker), which
 * also covers a teardown that never ran (SIGKILL, a Ctrl-C storm).
 */

/**
 * Recognisable prefixes so `scripts/sweep-test-residue.sh e2e-fake` can find a
 * previous run's leftovers; keep them in sync with that script.
 */
const TMP_PREFIX = 'delta-e2e-fake.';
const SOCKET_PREFIX = 'delta-e2e-fake-';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BACKEND_DIR = path.join(REPO_ROOT, 'backend');
const SERVER_BIN = path.join(BACKEND_DIR, 'target/debug/delta-server');
const FAKE_BIN = path.join(BACKEND_DIR, 'target/debug/fake-claude');
const FAKE_CODEX_BIN = path.join(BACKEND_DIR, 'target/debug/fake-codex');
const WEB_DIR = path.join(REPO_ROOT, 'frontend/packages/apps/web');
/** Vite's CLI entry, run with this Node so the process is Vite itself. */
const VITE_BIN = path.join(
  path.dirname(
    createRequire(path.join(WEB_DIR, 'package.json')).resolve(
      'vite/package.json',
    ),
  ),
  'bin/vite.js',
);
/**
 * Preloaded into the dev server: end the process (through Vite's own SIGTERM
 * shutdown) once its stdin reaches EOF, which happens when the worker holding
 * the other end dies. Vite registers a stdin `end` listener itself, but never
 * reads stdin, so on its own the EOF goes unnoticed and an orphaned dev server
 * keeps the port for good — observed after a run's Playwright processes were
 * killed.
 */
const EXIT_WITH_PARENT =
  "data:text/javascript,process.stdin.on('end', () => process.kill(process.pid, 'SIGTERM')).resume();";
/** The dev server's own address; the page reaches it as `localhost`. */
const DEV_SERVER_URL = `http://127.0.0.1:${DEV_SERVER_PORT}/`;
const BACKEND_HEALTH_URL = `http://127.0.0.1:${BACKEND_PORT}/health`;
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

/** Elevated, overridable log level (a caller-provided `RUST_LOG` wins). */
const SERVER_RUST_LOG = process.env.RUST_LOG ?? 'delta_usecase=debug,info';

/** The handle a spec uses to drive the server it is running against. */
export interface ServerHandle {
  /** The backend port the server (and the Vite proxy) is bound to. */
  readonly port: number;
  /**
   * The directory the fake writes its JSONL transcripts into (one
   * `<session-id>.jsonl` per launch), for a spec asserting on the exact bytes
   * the fake submitted.
   */
  readonly transcriptDir: string;
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
  /**
   * The servers not answering right now (`backend`, `dev server`): a process
   * that has exited, or one that does not answer its readiness URL.
   */
  findDown(): Promise<string[]>;
  /**
   * Bring back every server {@link findDown} reports — the backend with the
   * suite's shared configuration, as a bare `restart()` would — and return
   * which ones were restarted.
   */
  restoreDown(): Promise<string[]>;
  /**
   * Kill both servers and tmux and copy transcripts out. The run directory is
   * left behind as evidence; the next run's sweep removes it.
   */
  teardown(): Promise<void>;
}

/**
 * Create and return this boot's private directory under the artifact dir,
 * `boot-<N>/` with N one past the boots already recorded there, so a reboot
 * lands beside the previous boot's diagnostics instead of on top of them.
 */
function allocateBootDir(): string {
  fs.mkdirSync(ARTIFACT_DIR, { recursive: true });
  const boots = fs
    .readdirSync(ARTIFACT_DIR, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name.startsWith('boot-'));
  const dir = path.join(ARTIFACT_DIR, `boot-${boots.length + 1}`);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

/** Whether `url` answers 2xx within `timeoutMs`. */
async function answersOk(url: string, timeoutMs: number): Promise<boolean> {
  try {
    const res = await fetch(url, { signal: AbortSignal.timeout(timeoutMs) });
    return res.ok;
  } catch {
    return false;
  }
}

/**
 * Poll `url` until it answers ok, or throw with the log tail. `what` names the
 * server in the error.
 */
async function waitReady(
  what: string,
  url: string,
  isChildAlive: () => boolean,
  logPath: string,
  timeoutMs: number,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!isChildAlive()) {
      throw new Error(
        `${what} exited during startup. Log tail:\n${logTail(logPath)}`,
      );
    }
    if (await answersOk(url, 2_000)) {
      // The port was free before the spawn, so this is our child answering —
      // unless it died meanwhile and something else took the port.
      if (!isChildAlive()) {
        throw new Error(
          `${what} exited during startup, but ${url} answers: another process ` +
            `took the port.\n${portHolder(Number(new URL(url).port))}\nLog tail:\n${logTail(logPath)}`,
        );
      }
      return;
    }
    await sleep(100);
  }
  throw new Error(
    `${what} did not answer ${url} within ${timeoutMs / 1000}s. Log tail:\n${logTail(logPath)}`,
  );
}

/** Whether anything accepts a TCP connection on 127.0.0.1:`port`. */
function portAccepts(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = net.connect({ host: '127.0.0.1', port });
    socket.once('connect', () => {
      socket.destroy();
      resolve(true);
    });
    socket.once('error', () => resolve(false));
  });
}

/** The process listening on `port`, as lsof shows it. */
function portHolder(port: number): string {
  try {
    return execFileSync('lsof', ['-nP', `-iTCP:${port}`, '-sTCP:LISTEN'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
  } catch {
    return `(lsof could not name the process listening on port ${port})`;
  }
}

/**
 * Wait until nothing listens on `port` any more, so the server about to start
 * there binds it rather than failing to — or, worse, having its readiness poll
 * answered by whatever still holds it. A process this worker just killed
 * releases its port as it exits, so the wait is normally zero; anything still
 * listening after the grace period is not ours, and the error names it.
 */
async function waitPortReleased(what: string, port: number): Promise<void> {
  const deadline = Date.now() + 10_000;
  while (await portAccepts(port)) {
    if (Date.now() >= deadline) {
      throw new Error(
        `Cannot start ${what}: port ${port} is held by a process this run does ` +
          `not own — another e2e-fake run (in this or another checkout), or ` +
          `one a killed run left behind. Stop it and re-run.\n${portHolder(port)}`,
      );
    }
    await sleep(100);
  }
}

const isRunning = (proc: ChildProcess | null): proc is ChildProcess =>
  proc !== null && proc.exitCode === null && proc.signalCode === null;

/**
 * Start one server: wait for `port` to be free, spawn the process through
 * `launch` (handed the log file's descriptor for its stdout and stderr), and
 * wait until `readyUrl` answers. `launch` records the process before the wait,
 * so the caller can still kill one that never became ready.
 */
async function startServer(
  what: string,
  port: number,
  readyUrl: string,
  timeoutMs: number,
  logPath: string,
  launch: (logFd: number) => ChildProcess,
): Promise<void> {
  await waitPortReleased(what, port);
  const logFd = fs.openSync(logPath, 'a');
  try {
    const proc = launch(logFd);
    await waitReady(what, readyUrl, () => isRunning(proc), logPath, timeoutMs);
  } finally {
    fs.closeSync(logFd);
  }
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
 * Boot a fresh server for this worker: lay down the per-run temp dir and the
 * two scripted-agent wrappers (claude and codex), spawn generation 1, and wait
 * for `/health`. Returns the handle the fixture hands to specs.
 */
export async function bootServer(): Promise<ServerHandle> {
  const runDir = fs.mkdtempSync(path.join(os.tmpdir(), TMP_PREFIX));
  // The sweep at the start of a later run leaves this directory alone while
  // this worker is alive, and treats it as a dead run's evidence afterwards.
  fs.writeFileSync(path.join(runDir, 'owner.pid'), `${process.pid}\n`);
  const transcripts = path.join(runDir, 'transcripts');
  const pidFile = path.join(runDir, 'server.pids');
  const tmuxSocket = `${SOCKET_PREFIX}${process.pid}`;
  fs.mkdirSync(transcripts, { recursive: true });
  // Everything this boot preserves for CI goes here: its generation logs, and
  // the transcripts copied out on teardown.
  const bootDir = allocateBootDir();

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

  /** `server.log`, `server.2.log`, … — one file per start of `name`. */
  const logFile = (name: string, n: number): string =>
    path.join(bootDir, n === 1 ? `${name}.log` : `${name}.${n}.log`);

  let generation = 0;
  let child: ChildProcess | null = null;
  /** Per-spec environment layered over the defaults; see `restart`. */
  let envOverrides: Record<string, string> = {};

  const spawnGeneration = async (): Promise<void> => {
    generation += 1;
    await startServer(
      'delta-server',
      BACKEND_PORT,
      BACKEND_HEALTH_URL,
      30_000,
      logFile('server', generation),
      (logFd) => {
        const proc = spawn(SERVER_BIN, [], {
          stdio: ['ignore', logFd, logFd],
          env: {
            ...process.env,
            RUST_LOG: SERVER_RUST_LOG,
            DELTA_PORT: String(BACKEND_PORT),
            DELTA_AUTH_TOKEN: AUTH_TOKEN,
            DELTA_DATA_DIR: runDir,
            DELTA_TMUX_SOCKET: tmuxSocket,
            DELTA_CLAUDE_BIN: wrapper,
            DELTA_CODEX_BIN: codexWrapper,
            // Confine hook-reported transcript paths to where the fake actually
            // writes them (the same dir exported to the wrapper as
            // FAKE_CLAUDE_TRANSCRIPT_DIR); without this the backend would reject
            // the fake's transcript path as outside the default ~/.claude/projects
            // root and never ingest a turn. The backend mints its own hook secret
            // and renders it into the hook URLs the fake reads, so no hook-secret
            // env is needed here.
            DELTA_TRANSCRIPT_ROOT: transcripts,
            DELTA_LAUNCH_DEADLINE_MS: '3000',
            DELTA_PERMISSION_DECISION_TIMEOUT_MS: '3000',
            // The echo watchdog stays near its production generosity for the
            // shared suite: several specs deliberately hold a send `dispatched`
            // with no echo (cancel, restart) and must not have it retried or
            // parked out from under them. The spec that exercises the watchdog
            // shrinks this for its own server generation via `restart`.
            DELTA_ECHO_DEADLINE_MS: '60000',
            // Turn the background release check off, so the suite never reaches
            // GitHub (and never depends on what it answers).
            DELTA_RELEASE_FEED_URL: '',
            ...envOverrides,
          },
        });
        child = proc;
        if (proc.pid !== undefined) {
          fs.appendFileSync(pidFile, `${proc.pid}\n`);
        }
        return proc;
      },
    );
  };

  let devGeneration = 0;
  let devServer: ChildProcess | null = null;

  /**
   * Start the Vite dev server, proxied to the backend, and wait until it
   * serves the page.
   *
   * Its stdin is a pipe only this worker holds, and the process stops itself
   * when that pipe closes ({@link EXIT_WITH_PARENT}), so a worker killed
   * outright takes its dev server with it instead of leaving it on the port.
   */
  const startDevServer = async (): Promise<void> => {
    devGeneration += 1;
    await startServer(
      'the Vite dev server',
      DEV_SERVER_PORT,
      DEV_SERVER_URL,
      // A cold start pre-bundles dependencies.
      120_000,
      logFile('vite', devGeneration),
      (logFd) => {
        devServer = spawn(
          process.execPath,
          [
            '--import',
            EXIT_WITH_PARENT,
            VITE_BIN,
            '--port',
            String(DEV_SERVER_PORT),
            '--strictPort',
            // Bound to the loopback address explicitly, so the port check and
            // the readiness poll look exactly where it listens.
            '--host',
            '127.0.0.1',
          ],
          {
            cwd: WEB_DIR,
            stdio: ['pipe', logFd, logFd],
            env: {
              ...process.env,
              DELTA_PORT: String(BACKEND_PORT),
              DELTA_AUTH_TOKEN: AUTH_TOKEN,
            },
          },
        );
        return devServer;
      },
    );
  };

  /**
   * Send `signal` to `proc` and wait for it to exit. A process that has not
   * exited 5s after a gentler signal is killed outright.
   */
  const kill = async (
    proc: ChildProcess | null,
    signal: NodeJS.Signals,
  ): Promise<void> => {
    if (!isRunning(proc) || proc.pid === undefined) {
      return;
    }
    const done = new Promise<void>((resolve) => proc.once('exit', () => resolve()));
    proc.kill(signal);
    if (signal !== 'SIGKILL') {
      const exitedInTime = await Promise.race([
        done.then(() => true),
        sleep(5_000).then(() => false),
      ]);
      if (!exitedInTime) {
        proc.kill('SIGKILL');
      }
    }
    await done;
  };
  // The backend dies hard, as in the production incident the restart specs
  // model; the dev server is let to shut down (and stop its esbuild service).
  const killChild = (): Promise<void> => kill(child, 'SIGKILL');
  const killDevServer = (): Promise<void> => kill(devServer, 'SIGTERM');
  const backendDown = async (): Promise<boolean> =>
    !isRunning(child) || !(await answersOk(BACKEND_HEALTH_URL, 5_000));
  const devServerDown = async (): Promise<boolean> =>
    !isRunning(devServer) || !(await answersOk(DEV_SERVER_URL, 5_000));

  try {
    await startDevServer();
    await spawnGeneration();
  } catch (error) {
    await killChild();
    await killDevServer();
    throw error;
  }

  const restart = async (env: Record<string, string> = {}): Promise<void> => {
    envOverrides = env;
    await killChild();
    await spawnGeneration();
  };

  return {
    port: BACKEND_PORT,
    transcriptDir: transcripts,
    restart,
    async findDown(): Promise<string[]> {
      const down: string[] = [];
      if (await backendDown()) {
        down.push('backend');
      }
      if (await devServerDown()) {
        down.push('dev server');
      }
      return down;
    },
    async restoreDown(): Promise<string[]> {
      const restored: string[] = [];
      if (await devServerDown()) {
        await killDevServer();
        await startDevServer();
        restored.push('dev server');
      }
      if (await backendDown()) {
        await restart();
        restored.push('backend');
      }
      return restored;
    },
    async teardown(): Promise<void> {
      await killChild();
      await killDevServer();
      // Copy the fake transcripts out of the temp dir into this boot's
      // directory under the artifact dir CI uploads (best-effort — a run that
      // never spawned claude leaves none). The temp dir itself stays behind
      // until the next run's sweep, which may remove it.
      try {
        fs.cpSync(transcripts, path.join(bootDir, 'transcripts'), {
          recursive: true,
        });
      } catch {
        /* nothing to preserve */
      }
      // Kill the whole per-run tmux server: every pane this run spawned dies
      // with it; other sockets (the app's or `make dev`'s, another run) are
      // untouched.
      await new Promise<void>((resolve) => {
        const tmux = spawn('tmux', ['-L', tmuxSocket, 'kill-server'], {
          stdio: 'ignore',
        });
        tmux.on('error', () => resolve());
        tmux.on('exit', () => resolve());
      });
    },
  };
}
