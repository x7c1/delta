import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { connectPty, type PtyConnection } from '@delta/api-client';
import type { SessionId } from '@delta/model';
import { Panel } from '@delta/ui-kit';
import '@xterm/xterm/css/xterm.css';
import { isMockMode, wsUrl } from '../../config';
import { useThemeContext } from '../../hooks/themeContext';
import { useNavStore } from '../../store/navStore';
import {
  terminalBackground,
  terminalFontFamily,
  terminalFontSize,
} from '../../theme';

/**
 * What the focused session offers the terminal, which is not the same question
 * as whether it is open.
 *
 * A session's pane comes up before anything binds it, and that window is
 * exactly when the embedded terminal matters most: a launch can stop on an
 * interactive prompt (Claude Code's workspace-trust dialog) that fires no hook,
 * so nothing but a human at the pane can get it moving. So "may be attached to"
 * is wider than "is open", and the states that cannot be attached to differ in
 * what they should say — a session that is starting is not a session that was
 * closed.
 */
export type TerminalPaneState =
  /** The launch is accepted but has no pane yet: nothing to attach to. */
  | 'preparing'
  /** The pane is up and nothing has bound it: attachable and interactive. */
  | 'starting'
  /** Bound: the ordinary open session, attached exactly as it always was. */
  | 'open'
  /** Closed, and resumable. */
  | 'closed'
  /** The launch never came up: no pane, and nothing to resume. */
  | 'failed';

export interface TerminalPaneProps {
  /**
   * The focused session whose PTY pane to show. Null for a not-yet-bound New
   * session (no pane exists), in which case the terminal is disabled.
   */
  sessionId: SessionId | null;
  /** How far the focused session's pane has got (see {@link TerminalPaneState}). */
  paneState: TerminalPaneState;
  /**
   * Whether the focused session's provider offers an attachable terminal, read
   * from its capability profile (`GET /api/providers`) — never from the provider
   * id. A terminal-less provider (Codex's headless app-server) must NEVER open a
   * `/pty` bridge, so this gates the attach as authoritatively as
   * {@link TerminalPaneProps.paneState}: the enclosing pane is already withheld
   * for such a provider, and this keeps the connect itself capability-driven
   * even if the pane is ever mounted.
   */
  hasTerminal: boolean;
}

/** A live xterm instance bound to one session's `/pty` pane, kept alive while
 * the terminal is open even when another session is focused. */
interface PaneEntry {
  el: HTMLDivElement;
  term: Terminal;
  fit: FitAddon;
  connection: PtyConnection;
  observer: ResizeObserver;
  rafId: number;
  /** Set once the bridge socket closes (session closed or server gone) so a
   * later refocus rebuilds the entry instead of reusing a dead socket. */
  closed: boolean;
}

/**
 * The embedded xterm.js terminal for the focused session's `/pty` pane — the
 * access path for answering anything the TUI asks, which is why it attaches for
 * a session that is merely *starting* as well as an open one
 * ({@link TerminalPaneState}). In mock mode the PTY socket is not available, so
 * it renders an informational placeholder; so do the states with no pane behind
 * them.
 *
 * Each session gets its own xterm instance, created on first view and **kept
 * attached** while the terminal stays open — switching between *open* sessions
 * only shows a different instance, it never detaches and re-attaches. tmux
 * delivers a focus-out report to the pane's program (Claude's input) every time
 * a client detaches, which Claude renders as a stray blank line, so re-attaching
 * on every session switch made those blank lines pile up. Holding one persistent
 * attach per open session, exactly as a normal `tmux attach` would, keeps the
 * input clean. The one exception is a session that gets **closed**: its entry is
 * disposed so a later resume rebuilds against the fresh pane (see the effect).
 */
export function TerminalPane({
  sessionId,
  paneState,
  hasTerminal,
}: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const entriesRef = useRef<Map<SessionId, PaneEntry>>(new Map());
  const pendingTeardownRef = useRef<number | null>(null);
  const setTerminalOpen = useNavStore((state) => state.setTerminalOpen);
  // Subscribe to the resolved theme so a picker change repaints every live
  // xterm instance below without requiring a session detach + reattach.
  const { resolved: resolvedTheme } = useThemeContext();

  // The two states with a live pane behind them, and so the two the `/pty`
  // bridge resolves.
  const hasLivePane = paneState === 'open' || paneState === 'starting';
  const canAttach =
    !isMockMode() && hasLivePane && hasTerminal && sessionId !== null;

  // Show the focused session's pane, keeping the others attached but hidden.
  useEffect(() => {
    const entries = entriesRef.current;
    const parent = containerRef.current;
    if (!canAttach || sessionId === null || !parent) {
      // The focused session is known but has no live pane — it was closed, or
      // its launch failed. Drop its live entry now so a later resume (a Send or
      // the open button) rebuilds against the freshly-resumed pane. Relying on
      // the bridge socket's async `closed` flag races with the resume: if the
      // close event lands after this effect re-runs, the dead entry is reused
      // and the terminal stays blank until a manual reload. Other early-return
      // reasons (mock mode, a New session with no pane, the container not yet
      // mounted) keep their entries hidden.
      //
      // A session that is merely `preparing` is neither dropped nor hidden. Its
      // ordinary form has no entry to drop (there was no pane to build one
      // against), but it is also what the focused session reads as for the
      // moment between the bind landing and the session row saying so: the
      // live event clears the starting-pane mark at once, while `open` comes
      // off the row, which only flips after its refetch. Tearing the terminal
      // down for that blink would close the `/pty` socket the instant the user
      // answered the prompt they attached for — detaching the tmux client (the
      // stray blank line this component exists to avoid), typing the pre-attach
      // input wipe into the freshly-bound pane on the way back, and dropping
      // whatever they were mid-way through typing.
      const paneIsGone = paneState === 'closed' || paneState === 'failed';
      if (sessionId !== null && paneIsGone && !isMockMode()) {
        const closedEntry = entries.get(sessionId);
        if (closedEntry) {
          disposeEntry(closedEntry);
          entries.delete(sessionId);
        }
      }
      for (const [id, entry] of entries) {
        entry.el.style.display =
          id === sessionId && paneState === 'preparing' ? 'block' : 'none';
      }
      return;
    }

    let entry = entries.get(sessionId);
    if (entry && entry.closed) {
      // The session's previous bridge socket died (it was closed and resumed);
      // drop the stale instance so it is rebuilt against the fresh pane.
      disposeEntry(entry);
      entries.delete(sessionId);
      entry = undefined;
    }
    if (!entry) {
      entry = createEntry(sessionId, parent);
      entries.set(sessionId, entry);
    }

    for (const [id, current] of entries) {
      current.el.style.display = id === sessionId ? 'block' : 'none';
    }
    entry.fit.fit();
    // `paneState` rather than `hasLivePane` alone: the branch above tells the
    // three pane-less states apart (only `closed` and `failed` tear an entry
    // down), so a move between two of them has to re-run it.
  }, [canAttach, paneState, sessionId]);

  // When the active theme changes, repaint every live xterm: each Terminal
  // reads its background once at construction (see `createEntry`), so a
  // picker flip would otherwise leave the canvas on the previous color until
  // the session is detached and reattached. Reassigning `options.theme`
  // pushes the new value into xterm's renderer, which clears and redraws the
  // visible buffer in place. `resolvedTheme` is the dep — the live id from
  // `useThemeContext`, which already reacts to both an explicit pick and a
  // `prefers-color-scheme` flip under the 'system' preference. Reading
  // `terminalBackground()` inside the effect (rather than passing it in)
  // ensures the freshly applied `:root[data-theme="…"]` block is observed,
  // since the bridging `<html>` attribute is written by ThemeProvider before
  // this effect runs.
  useEffect(() => {
    for (const entry of entriesRef.current.values()) {
      entry.term.options.theme = {
        ...entry.term.options.theme,
        background: terminalBackground(),
      };
    }
  }, [resolvedTheme]);

  // Detach everything only when the terminal itself closes (this unmounts).
  //
  // The teardown is deferred to a macrotask so React StrictMode's dev-only
  // mount → unmount → mount does not destroy the just-built terminals: the
  // immediate remount cancels the pending teardown, so the entries (which this
  // component deliberately keeps alive while open) survive. Without this the
  // throwaway unmount would close each `/pty` socket while it is still
  // connecting (a "closed before the connection is established" warning) and
  // dispose each xterm before its queued `open()` timer fires (an uncaught
  // "reading 'dimensions'" error). A real unmount has nothing to cancel it, so
  // the teardown runs on the next tick.
  useEffect(() => {
    const entries = entriesRef.current;
    if (pendingTeardownRef.current !== null) {
      window.clearTimeout(pendingTeardownRef.current);
      pendingTeardownRef.current = null;
    }
    return () => {
      pendingTeardownRef.current = window.setTimeout(() => {
        pendingTeardownRef.current = null;
        for (const entry of entries.values()) {
          disposeEntry(entry);
        }
        entries.clear();
      }, 0);
    };
  }, []);

  // Message shown instead of the live terminal when no pane can be shown. Each
  // state says what is actually true of it: a session that is starting is told
  // to wait, not told to resume something that was never closed.
  const unavailableNote = isMockMode()
    ? 'The terminal attaches to the live PTY bridge. It is unavailable in mock mode (no backend). Run against the Delta server to use it for answering permission prompts in the TUI.'
    : sessionId === null
      ? 'No session is attached yet. Start a session, then its terminal appears here.'
      : NOTE_BY_PANE_STATE[paneState];

  return (
    <Panel
      className="border-l border-border-default"
      bodyClassName="bg-terminal-bg"
    >
      {/* The per-session xterm elements are appended into this container; the
          note overlays it only while no pane is attachable. */}
      <div ref={containerRef} className="relative h-full w-full">
        <button
          type="button"
          onClick={() => setTerminalOpen(false)}
          aria-label="Close terminal"
          title="Close terminal"
          className="absolute right-2 top-2 z-10 rounded bg-terminal-overlay/60 px-1.5 py-0.5 text-secondary leading-none text-terminal-fg opacity-60 transition hover:bg-terminal-overlay-hover hover:text-terminal-fg-strong hover:opacity-100 focus-visible:opacity-100"
        >
          »
        </button>
        {unavailableNote && (
          <p className="p-3 text-caption text-terminal-fg">{unavailableNote}</p>
        )}
      </div>
    </Panel>
  );
}

/**
 * What to say for each state with no pane to show, and `null` for the two that
 * have one (the live terminal is the message).
 */
const NOTE_BY_PANE_STATE: Record<TerminalPaneState, string | null> = {
  // The launch is being prepared — a worktree checkout, a settings write — and
  // its pane does not exist yet. Observed preparations run from instant to
  // several seconds, so this is a real state the user sees, and the honest
  // thing to say is that there is nothing yet rather than nothing at all.
  preparing:
    'This session is still starting up. Its terminal appears as soon as the agent is running.',
  starting: null,
  open: null,
  closed: 'This session is closed. Resume it to attach its terminal.',
  failed: 'This session never started, so it has no terminal.',
};

/** Build a live xterm bound to `sessionId`'s pane, appended into `parent`. */
function createEntry(sessionId: SessionId, parent: HTMLDivElement): PaneEntry {
  const el = document.createElement('div');
  el.className = 'absolute inset-0';
  parent.appendChild(el);

  const term = new Terminal({
    convertEol: true,
    // The design tokens own the stack, the size, and the background
    // (tailwind.config.js `fontFamily.terminal` / `--delta-text-terminal` /
    // `--delta-color-terminal-bg`); xterm takes them as JavaScript options, so
    // they are read off the document here instead of being restated. See the
    // config for the per-OS font reasoning.
    fontFamily: terminalFontFamily(),
    fontSize: terminalFontSize(),
    theme: { background: terminalBackground() },
    // `term.unicode` is a proposed API that the Unicode 11 addon touches, so it
    // must be opted into or `loadAddon`/`activeVersion` throws at attach time.
    allowProposedApi: true,
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  // Use the Unicode 11 width table so emoji, full-width, and CJK glyphs are
  // measured as the correct number of cells; the default Unicode 6 table
  // under-counts them and shifts subsequent columns.
  term.loadAddon(new Unicode11Addon());
  term.unicode.activeVersion = '11';
  term.open(el);
  fit.fit();

  // Pass { stream: true } so that an incomplete multi-byte UTF-8 sequence
  // split across WebSocket frame boundaries is held in the decoder's internal
  // buffer and completed by the next chunk. Without this, each decode() call
  // flushes the buffer, replacing any trailing incomplete byte(s) with U+FFFD.
  // The PTY output is a continuous stream, so there is no meaningful "end":
  // any bytes still buffered when the socket closes are silently dropped, which
  // is acceptable — a final incomplete sequence would be garbled either way.
  const decoder = new TextDecoder();
  const entry: PaneEntry = {
    el,
    term,
    fit,
    connection: connectPty({
      url: wsUrl('/pty'),
      sessionId,
      onData: (chunk) => term.write(decoder.decode(chunk, { stream: true })),
      onStatus: (status) => {
        if (status === 'closed') {
          entry.closed = true;
        } else if (status === 'open') {
          // A resize sent before the socket is OPEN is dropped, so push the
          // current size once the bridge (re)connects to sync the server PTY.
          entry.connection.resize(entry.term.rows, entry.term.cols);
        }
      },
    }),
    observer: undefined as unknown as ResizeObserver,
    rafId: 0,
    closed: false,
  };
  term.onData((data) => entry.connection.send(data));
  // Push every fit-driven size change to the server so tmux and the pane
  // program follow the browser terminal's dimensions.
  term.onResize(({ rows, cols }) => entry.connection.resize(rows, cols));

  // Reflow on container resize (pane drag / window resize), coalesced onto one
  // animation frame to avoid thrashing fit() during a drag.
  entry.observer = new ResizeObserver(() => {
    if (entry.rafId !== 0) {
      return;
    }
    entry.rafId = window.requestAnimationFrame(() => {
      entry.rafId = 0;
      entry.fit.fit();
    });
  });
  entry.observer.observe(el);
  return entry;
}

/** Tear down a pane entry's socket, terminal, observer, and DOM node. */
function disposeEntry(entry: PaneEntry): void {
  if (entry.rafId !== 0) {
    window.cancelAnimationFrame(entry.rafId);
  }
  entry.observer.disconnect();
  entry.connection.close();
  entry.term.dispose();
  entry.el.remove();
}
