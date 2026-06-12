import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { connectPty, type PtyConnection } from '@delta/api-client';
import type { SessionId } from '@delta/model';
import { Button, Panel } from '@delta/ui-kit';
import '@xterm/xterm/css/xterm.css';
import { isMockMode, wsUrl } from '../../config';
import { useNavStore } from '../../store/navStore';
import { terminalBackground, terminalFontFamily } from '../../theme';

export interface TerminalPaneProps {
  /**
   * The focused session whose PTY pane to show. Null for a not-yet-bound New
   * session (no pane exists), in which case the terminal is disabled.
   */
  sessionId: SessionId | null;
  /** Whether the focused session is open (its pane is attachable). */
  attachable: boolean;
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
 * The embedded xterm.js terminal for the focused session's `/pty` pane. It is
 * the access path for answering permission prompts in the real TUI. In mock
 * mode the PTY socket is not available, so it renders an informational
 * placeholder; it is also disabled for a closed or not-yet-registered session.
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
export function TerminalPane({ sessionId, attachable }: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const entriesRef = useRef<Map<SessionId, PaneEntry>>(new Map());
  const pendingTeardownRef = useRef<number | null>(null);
  const setTerminalOpen = useNavStore((state) => state.setTerminalOpen);

  const canAttach = !isMockMode() && attachable && sessionId !== null;

  // Show the focused session's pane, keeping the others attached but hidden.
  useEffect(() => {
    const entries = entriesRef.current;
    const parent = containerRef.current;
    if (!canAttach || sessionId === null || !parent) {
      // The focused session is known but not attachable — it was closed. Drop
      // its live entry now so a later resume (a Send or the open button)
      // rebuilds against the freshly-resumed pane. Relying on the bridge
      // socket's async `closed` flag races with the resume: if the close event
      // lands after this effect re-runs, the dead entry is reused and the
      // terminal stays blank until a manual reload. Other early-return reasons
      // (mock mode, a New session with no pane, the container not yet mounted)
      // keep their entries hidden.
      if (sessionId !== null && !attachable && !isMockMode()) {
        const closedEntry = entries.get(sessionId);
        if (closedEntry) {
          disposeEntry(closedEntry);
          entries.delete(sessionId);
        }
      }
      for (const entry of entries.values()) {
        entry.el.style.display = 'none';
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
  }, [canAttach, sessionId]);

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

  // Message shown instead of the live terminal when no pane can be shown.
  const unavailableNote = isMockMode()
    ? 'The terminal attaches to the live PTY bridge. It is unavailable in mock mode (no backend). Run against the Delta server to use it for answering permission prompts in the TUI.'
    : sessionId === null
      ? 'No session is attached yet. Start a session, then its terminal appears here.'
      : !attachable
        ? 'This session is closed. Resume it to attach its terminal.'
        : null;

  return (
    <Panel
      className="border-l border-slate-200"
      header={
        <div className="flex items-center justify-between">
          <span className="text-sm font-semibold text-slate-700">Terminal</span>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setTerminalOpen(false)}
            aria-label="Close terminal"
          >
            ✕
          </Button>
        </div>
      }
      bodyClassName="bg-terminal-bg"
    >
      {/* The per-session xterm elements are appended into this container; the
          note overlays it only while no pane is attachable. */}
      <div ref={containerRef} className="relative h-full w-full">
        {unavailableNote && (
          <p className="p-3 text-xs text-slate-300">{unavailableNote}</p>
        )}
      </div>
    </Panel>
  );
}

/** Build a live xterm bound to `sessionId`'s pane, appended into `parent`. */
function createEntry(sessionId: SessionId, parent: HTMLDivElement): PaneEntry {
  const el = document.createElement('div');
  el.className = 'absolute inset-0';
  parent.appendChild(el);

  const term = new Terminal({
    convertEol: true,
    // The design tokens own the stack and the background (tailwind.config.js
    // `fontFamily.terminal` / `--delta-terminal-bg`); xterm takes them as
    // JavaScript options, so they are read off the document here instead of
    // being restated. See the config for the per-OS font reasoning.
    fontFamily: terminalFontFamily(),
    fontSize: 13,
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
