import { createContext, useContext, useState, type ReactNode } from 'react';
import type { EraseResponse } from '@delta/wire-gen';
import { ErasedPage } from './ErasedPage';

/** Replaces the app with the erase report. */
type ShowErased = (report: EraseResponse) => void;

const ErasedContext = createContext<ShowErased | null>(null);

/**
 * Renders its children until an erase succeeds, then only the report.
 *
 * The server stops right after it answers the erase, so everything the app
 * shows from then on would fail: the live channels (`/ws`, `/comms`) would
 * start reconnecting and every query would error. Replacing the children
 * unmounts all of it — the reconnection UI included — and leaves a static page
 * built from the response alone.
 */
export function ErasedGate({ children }: { children: ReactNode }) {
  const [report, setReport] = useState<EraseResponse | null>(null);
  if (report !== null) {
    return <ErasedPage report={report} />;
  }
  return <ErasedContext.Provider value={setReport}>{children}</ErasedContext.Provider>;
}

/** The function that replaces the app with an erase report. */
export function useShowErased(): ShowErased {
  const show = useContext(ErasedContext);
  if (show === null) {
    throw new Error('useShowErased must be used inside <ErasedGate>');
  }
  return show;
}
