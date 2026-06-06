import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { connectPty, type PtyConnection } from '@delta/api-client';
import { Button, Panel } from '@delta/ui-kit';
import '@xterm/xterm/css/xterm.css';
import { isMockMode, wsUrl } from '../../config';
import { useNavStore } from '../../store/navStore';

/**
 * The embedded xterm.js terminal attached to `/pty`. It is the access path for
 * answering permission prompts in the real TUI. In mock mode the PTY socket is
 * not available, so it renders an informational placeholder instead.
 */
export function TerminalPane() {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const setTerminalOpen = useNavStore((state) => state.setTerminalOpen);

  useEffect(() => {
    if (isMockMode() || !containerRef.current) {
      return;
    }
    const term = new Terminal({
      convertEol: true,
      fontFamily: 'monospace',
      fontSize: 13,
      theme: { background: '#0f172a' },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    fit.fit();

    const decoder = new TextDecoder();
    let connection: PtyConnection | null = null;
    connection = connectPty({
      url: wsUrl('/pty'),
      onData: (chunk) => term.write(decoder.decode(chunk)),
    });
    term.onData((data) => connection?.send(data));

    const onResize = () => fit.fit();
    window.addEventListener('resize', onResize);

    return () => {
      window.removeEventListener('resize', onResize);
      connection?.close();
      term.dispose();
    };
  }, []);

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
      {isMockMode() ? (
        <p className="p-3 text-xs text-slate-300">
          The terminal attaches to the live PTY bridge. It is unavailable in
          mock mode (no backend). Run against the Delta server to use it for
          answering permission prompts in the TUI.
        </p>
      ) : (
        <div ref={containerRef} className="h-full w-full" />
      )}
    </Panel>
  );
}
