// Pure domain types for Delta. These mirror the wire JSON shapes documented in
// docs/guides/api.md exactly. No React, no fetch, no side effects here.

/** String identifier for a session. */
export type SessionId = string;

export type SessionStatus = 'active' | 'ended';

export interface Session {
  id: SessionId;
  cwd: string;
  transcript_path: string;
  title: string | null;
  status: SessionStatus;
  created_at: string;
}
