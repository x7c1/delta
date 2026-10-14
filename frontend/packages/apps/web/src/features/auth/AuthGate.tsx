import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { hasFragmentCode, type AuthState } from './establishAuth';
import { ReopenScreen } from './ReopenScreen';

/**
 * Renders the app while the page holds a token the server accepts, and the
 * {@link ReopenScreen} otherwise.
 *
 * `initial` is what the boot settled (`establishAuth`). `children` receives
 * the callback to hand the API client as its `onUnauthorized`: the first `401`
 * — the server restarted with a new token — swaps the whole app for the
 * screen, so a failing workspace never shows behind it.
 *
 * A link with a new code opened in this same tab only changes the fragment
 * (the page already removed the previous one), which a browser does without
 * loading the page again, so the boot that trades the code would never run.
 * The gate therefore reloads the page (`reload`) when the fragment gains a
 * code.
 */
export function AuthGate({
  initial,
  children,
  reload = () => window.location.reload(),
}: {
  initial: AuthState;
  children: (onUnauthorized: () => void) => ReactNode;
  reload?: () => void;
}) {
  const [state, setState] = useState<AuthState>(initial);
  const onUnauthorized = useCallback(
    () => setState({ kind: 'token-refused' }),
    [],
  );
  useEffect(() => {
    const onHashChange = () => {
      if (hasFragmentCode()) {
        reload();
      }
    };
    window.addEventListener('hashchange', onHashChange);
    return () => window.removeEventListener('hashchange', onHashChange);
  }, [reload]);
  if (state.kind !== 'ready') {
    return <ReopenScreen reason={state} />;
  }
  return <>{children(onUnauthorized)}</>;
}
