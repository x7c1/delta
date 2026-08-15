import type { StateCreator } from 'zustand';
import type { PullRequest } from '@delta/wire-gen';
import type { EventReducer } from './eventReducer';

/**
 * The clone the user asked for and is still waiting on.
 *
 * At most one at a time, by design: the intent is "clone this so I can start a
 * session on THAT PR", and there is only one composer to pre-fill. A second
 * request supersedes the first rather than queueing behind it.
 */
export interface CloneIntent {
  /** The PR whose repository is being cloned — the row that auto-continues. */
  pr: PullRequest;
  /**
   * `<clone_root>/<repo_name>`: where the server said it would land. This is the
   * intent's identity, because it is what the completion event names, and it is
   * also the workdir the auto-continue uses — so a landed clone needs no refetch
   * before the session can be composed.
   */
  destination: string;
}

/** A clone that finished while its intent was still the active one. */
export interface CloneCompletion {
  pr: PullRequest;
  destination: string;
}

/** A clone that failed while its intent was still the active one. */
export interface CloneFailure {
  pr: PullRequest;
  /** `gh`'s own words, shown inline on the row. */
  message: string;
}

/**
 * The state this module's actions and reducers read: only its own three fields.
 */
type ClonesState = Pick<
  ClonesSlice,
  'cloneIntent' | 'cloneCompletion' | 'cloneFailure'
>;

export interface ClonesSlice {
  /** The clone currently in flight for this dialog, or `null`. */
  cloneIntent: CloneIntent | null;
  /**
   * Set when the active intent's clone landed, and cleared by whoever consumes
   * it. A one-shot signal rather than durable state: the PR tab reads it, walks
   * the user into the PR pick, and clears it, so re-mounting the tab later
   * cannot replay an auto-continue the user has moved on from.
   */
  cloneCompletion: CloneCompletion | null;
  /** Set when the active intent's clone failed. Cleared on the next request. */
  cloneFailure: CloneFailure | null;

  /**
   * Record that a clone was requested for `pr` into `destination`. Supersedes
   * any previous intent and clears the last failure, since the row that showed
   * it is now retrying.
   */
  startCloneIntent: (intent: CloneIntent) => void;
  /**
   * Forget the intent and everything derived from it.
   *
   * This is what "the user moved on" means: the new-session dialog closed, the
   * PR tab was left, or another pick took over. After it, a clone that lands
   * still flips the row (the event router refetches regardless) but never
   * touches the composer — which is the whole point, since the composer may now
   * hold something the user chose deliberately.
   */
  clearCloneIntent: () => void;
  /** Consume the one-shot completion signal. */
  clearCloneCompletion: () => void;
}

export const createClonesSlice: StateCreator<
  ClonesState & ClonesSlice,
  [],
  [],
  ClonesSlice
> = (set) => ({
  cloneIntent: null,
  cloneCompletion: null,
  cloneFailure: null,

  startCloneIntent: (intent) =>
    set(() => ({
      cloneIntent: intent,
      cloneCompletion: null,
      cloneFailure: null,
    })),

  clearCloneIntent: () =>
    set(() => ({
      cloneIntent: null,
      cloneCompletion: null,
      cloneFailure: null,
    })),

  clearCloneCompletion: () => set(() => ({ cloneCompletion: null })),
});

/**
 * A clone landed.
 *
 * Only the *active* intent's clone is recorded. Anything else — a clone the user
 * has since navigated away from, or one another browser tab asked for — is
 * ignored here on purpose: the event router still refetches the lists on every
 * such event, so the row flips either way; what must not happen is a finished
 * clone reaching in and rewriting composer state the user has moved past.
 */
export const reduceRepositoryCloneCompleted: EventReducer<
  ClonesState,
  'repository_clone_completed'
> = (state, event) => {
  const intent = state.cloneIntent;
  if (intent === null || intent.destination !== event.destination_path) {
    return state;
  }
  return {
    cloneIntent: null,
    cloneCompletion: { pr: intent.pr, destination: event.destination_path },
  };
};

/**
 * A clone failed. Same active-intent gate as the completion: the message belongs
 * to the row the user is looking at, and there is no row to show it on once the
 * intent is gone. Retrying is simply clicking again.
 */
export const reduceRepositoryCloneFailed: EventReducer<
  ClonesState,
  'repository_clone_failed'
> = (state, event) => {
  const intent = state.cloneIntent;
  if (intent === null || intent.destination !== event.destination_path) {
    return state;
  }
  return {
    cloneIntent: null,
    cloneFailure: { pr: intent.pr, message: event.message },
  };
};
