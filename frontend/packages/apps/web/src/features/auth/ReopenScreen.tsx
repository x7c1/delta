import type { AuthState } from './establishAuth';

/** The states that show {@link ReopenScreen} instead of the app. */
export type ReopenReason = Exclude<AuthState, { kind: 'ready' }>;

/** Why this page holds no usable token, in the user's terms. */
function explain(reason: ReopenReason): string {
  switch (reason.kind) {
    case 'missing':
      return 'This page was opened without a link from the Delta server.';
    case 'code-refused':
      return 'This link has already been used or has expired. Each link opens Delta once, within 10 minutes.';
    case 'token-refused':
      return 'The Delta server was restarted, so this page needs a new link.';
    case 'exchange-failed':
      return `Delta could not open this page: ${reason.message}`;
  }
}

/**
 * Shown instead of the app while the page holds no token the server accepts:
 * says why, and how to get a link that opens Delta — the one the server
 * printed, a fresh one from `delta-server open-url` (`make open-url` on the
 * Vite dev server), or relaunching the desktop app.
 */
export function ReopenScreen({ reason }: { reason: ReopenReason }) {
  const command = `delta-server open-url --origin ${window.location.origin}`;
  return (
    <main
      className="mx-auto flex h-full max-w-2xl flex-col gap-4 overflow-auto px-6 py-8 text-secondary text-fg"
      data-testid="reopen-screen"
    >
      <h1 className="text-body font-semibold">Open Delta from a new link</h1>
      <p role="status">{explain(reason)}</p>
      <p>
        Open the link <span className="font-mono">delta-server</span> printed
        when it started, or print a new one without restarting the server:
      </p>
      <pre
        className="overflow-auto whitespace-pre-wrap break-all rounded-sm bg-surface-sunken px-2 py-1 font-mono text-code text-fg"
        data-testid="reopen-command"
      >
        {command}
      </pre>
      {/* The Vite dev server is `make dev`'s: its delta-server runs under the
          dev identifier, whose token file a bare `delta-server open-url` does
          not look in. */}
      {import.meta.env.DEV && (
        <p className="text-fg-muted">
          Under <span className="font-mono">make dev</span>, run{' '}
          <span className="font-mono">make open-url</span> in the repository
          instead.
        </p>
      )}
      <p className="text-fg-muted">
        In the desktop app, quit Delta and open it again.
      </p>
    </main>
  );
}
