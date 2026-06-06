import { useEffect, useState } from 'react';

/**
 * Track whether a CSS media query currently matches, re-rendering when it
 * changes. Use it to branch React layout on breakpoints (e.g. Tailwind's `lg`
 * via `'(min-width: 1024px)'`) instead of relying on CSS classes alone.
 *
 * Returns `false` during server-side rendering where `window` is absent.
 */
export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() =>
    typeof window === 'undefined' ? false : window.matchMedia(query).matches,
  );

  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }
    const mql = window.matchMedia(query);
    const onChange = () => setMatches(mql.matches);
    // Sync immediately in case the query changed between render and effect.
    onChange();
    mql.addEventListener('change', onChange);
    return () => mql.removeEventListener('change', onChange);
  }, [query]);

  return matches;
}
