import { create } from 'zustand';
import type { LaunchTrustResponse } from '@delta/wire-gen';

/** A launch-trust confirmation waiting for the user's answer. */
export interface PendingLaunchTrust {
  /** What the server said about the launch: why it asks, and the diffs. */
  check: LaunchTrustResponse;
  /** Settle the question: `true` to launch and trust, `false` to cancel. */
  resolve: (accepted: boolean) => void;
}

interface LaunchTrustState {
  /** The confirmation on screen, or `null` when none is being asked. */
  pending: PendingLaunchTrust | null;
  /**
   * Show the confirmation for `check` and resolve with the user's answer. A
   * question still on screen when a new one is asked is cancelled first, so
   * no caller is ever left waiting on an answer that cannot come.
   */
  ask: (check: LaunchTrustResponse) => Promise<boolean>;
  /** Answer the confirmation on screen (no-op when none is). */
  answer: (accepted: boolean) => void;
}

/**
 * The one launch-trust confirmation the app can show at a time.
 *
 * A store rather than component state because two unrelated places start a new
 * session — the composer's Send and a failed session's Retry — and both must
 * ask the same question in the same dialog, which is mounted once
 * (`LaunchTrustDialog`).
 */
export const useLaunchTrustStore = create<LaunchTrustState>((set, get) => ({
  pending: null,
  ask: (check) =>
    new Promise<boolean>((resolve) => {
      get().pending?.resolve(false);
      set({ pending: { check, resolve } });
    }),
  answer: (accepted) => {
    const pending = get().pending;
    if (pending === null) {
      return;
    }
    set({ pending: null });
    pending.resolve(accepted);
  },
}));
