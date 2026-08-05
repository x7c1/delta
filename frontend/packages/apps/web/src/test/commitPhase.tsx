import { Profiler, type ReactNode } from 'react';

/**
 * Test-only hook into React's commit phase.
 *
 * `Profiler.onRender` fires for every commit that touched its subtree, after
 * the DOM has been updated but before that commit's passive effects run. That
 * is the only lever a test has on the window components have to survive in
 * production: React defers its passive flush to a later task whenever a commit
 * overruns the scheduler's frame budget, so on a loaded machine a freshly
 * rendered row is on screen and clickable while the effect that would seed a
 * default value is still queued. Reproducing that ordering here needs no
 * timers, no fake clock, and no reliance on machine load.
 */
export function OnCommit({
  onCommit,
  children,
}: {
  onCommit?: () => void;
  children: ReactNode;
}) {
  return (
    <Profiler id="on-commit" onRender={() => onCommit?.()}>
      {children}
    </Profiler>
  );
}

/**
 * Builds an {@link OnCommit} callback that clicks the first element matching
 * `selector` whose text contains `label`, on the first commit where such an
 * element exists — i.e. from inside the commit that put it on screen, ahead of
 * that commit's passive effects. Later commits are ignored, so exactly one
 * click is dispatched.
 */
export function clickDuringCommit(selector: string, label: string): () => void {
  let clicked = false;
  return () => {
    if (clicked) {
      return;
    }
    const candidates = Array.from(
      document.querySelectorAll<HTMLElement>(selector),
    );
    const target = candidates.find((element) =>
      element.textContent?.includes(label),
    );
    if (!target) {
      return;
    }
    clicked = true;
    target.click();
  };
}
