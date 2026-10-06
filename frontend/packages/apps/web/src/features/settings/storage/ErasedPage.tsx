import type { EraseResponse } from '@delta/wire-gen';
import { KeptItemList } from './KeptItemList';

/** `1 session`, `12 sessions`. */
function countOf(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? '' : 's'}`;
}

/**
 * What is left of the app after Settings → Storage → Erase everything: what
 * was removed, what was kept and why, and that Delta has stopped. Static —
 * built from the erase's response alone, since the server is gone.
 */
export function ErasedPage({ report }: { report: EraseResponse }) {
  const { removed, kept } = report;
  return (
    <main
      className="mx-auto flex h-full max-w-2xl flex-col gap-4 overflow-auto px-6 py-8 text-secondary text-fg"
      data-testid="erased-page"
    >
      <h1 className="text-body font-semibold">Delta erased what it created</h1>
      <section className="flex flex-col gap-1" data-testid="erased-removed">
        <h2 className="font-medium">Removed</h2>
        <p>{countOf(removed.sessions, 'session')}.</p>
        <PathList title="Worktrees" items={removed.worktrees} />
        <PathList title="Branches" items={removed.branches} />
        {/* Deleted after this response, and the directory itself only when
            nothing else is left in it (on Ubuntu the desktop app's webview
            storage is), so the page cannot say it is gone. */}
        <p>
          Delta&apos;s files in the data directory{' '}
          <span className="font-mono">{removed.data_dir}</span>; the directory
          itself stays if anything else is left in it.
        </p>
      </section>
      <section className="flex flex-col gap-1">
        <h2 className="font-medium">Kept</h2>
        {kept.length === 0 ? (
          <p className="text-fg-muted">Nothing: none of it held work.</p>
        ) : (
          <>
            <p className="text-fg-muted">
              These hold work, or Delta could not tell that they do not. Remove
              them yourself if you no longer need them.
            </p>
            <KeptItemList kept={kept} testId="erased-kept" />
          </>
        )}
      </section>
      <p className="font-medium" role="status">
        Delta has stopped. Close this tab.
      </p>
    </main>
  );
}

function PathList({ title, items }: { title: string; items: string[] }) {
  if (items.length === 0) {
    return null;
  }
  return (
    <>
      <p className="text-fg-muted">{title}:</p>
      <ul className="flex flex-col gap-0.5 font-mono text-code text-fg-muted">
        {items.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
    </>
  );
}
