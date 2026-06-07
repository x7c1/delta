import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { connectPty, type PtyConnection } from '@delta/api-client';
import type { SessionId } from '@delta/model';
import { Button, Panel } from '@delta/ui-kit';
import '@xterm/xterm/css/xterm.css';
import { isMockMode, wsUrl } from '../../config';
import { useNavStore } from '../../store/navStore';

export interface TerminalPaneProps {
  /**
   * The focused session whose PTY pane to attach to. Null for a not-yet-bound
   * New session (no pane exists), in which case the terminal is disabled.
   */
  sessionId: SessionId | null;
  /** Whether the focused session is open (its pane is attachable). */
  attachable: boolean;
}

/**
 * The embedded xterm.js terminal attached to the focused session's `/pty` pane.
 * It is the access path for answering permission prompts in the real TUI. In
 * mock mode the PTY socket is not available, so it renders an informational
 * placeholder; it is also disabled when the focused session is closed or a
 * not-yet-registered New session (no pane to attach).
 */
export function TerminalPane({ sessionId, attachable }: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const setTerminalOpen = useNavStore((state) => state.setTerminalOpen);

  const canAttach = !isMockMode() && attachable && sessionId !== null;

  useEffect(() => {
    if (!canAttach || sessionId === null || !containerRef.current) {
      return;
    }
    const container = containerRef.current;
    const term = new Terminal({
      convertEol: true,
      fontFamily: 'monospace',
      fontSize: 13,
      theme: { background: '#0f172a' },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container);
    fit.fit();

    const decoder = new TextDecoder();
    let connection: PtyConnection | null = null;
    connection = connectPty({
      url: wsUrl('/pty'),
      sessionId,
      onData: (chunk) => term.write(decoder.decode(chunk)),
    });
    term.onData((data) => connection?.send(data));

    // Reflow the terminal whenever its container changes size — covers the
    // resizable pane drag as well as window resizes. Coalesce bursts onto a
    // single animation frame to avoid thrashing fit() during a drag.
    let rafId = 0;
    const scheduleFit = () => {
      if (rafId !== 0) {
        return;
      }
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        fit.fit();
      });
    };
    const observer = new ResizeObserver(scheduleFit);
    observer.observe(container);

    return () => {
      if (rafId !== 0) {
        window.cancelAnimationFrame(rafId);
      }
      observer.disconnect();
      connection?.close();
      term.dispose();
    };
    // Reattach when the focused session changes or it becomes (un)attachable.
  }, [canAttach, sessionId]);

  // Message shown instead of the live terminal when no pane can be attached.
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
      bodyClassName="bg-slate-900"
    >
      {unavailableNote ? (
        <p className="p-3 text-xs text-slate-300">{unavailableNote}</p>
      ) : (
        <div ref={containerRef} className="h-full w-full" />
      )}
    </Panel>
  );
}
