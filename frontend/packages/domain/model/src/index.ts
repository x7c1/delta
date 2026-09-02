// Frontend-side domain helpers for Delta. No React, no fetch, no side
// effects, and no dependency on any other workspace package.
//
// The wire JSON shapes (documented in docs/guides/api/shapes.md) are NOT defined
// here: they are generated from the backend's `delta-wire` crate into
// @delta/wire-gen. This package keeps only what the frontend adds on top —
// identifier aliases and pure view-model helpers like the thread tree.

export type { SessionId, ThreadId, MessageUuid } from './ids';
export {
  buildThreadTree,
  threadAncestry,
  type ThreadLike,
  type ThreadNode,
} from './thread-tree';
export {
  MAIN_THREAD_DISPLAY_NAME,
  emptyTitleFallback,
  threadDisplayName,
  threadTooltip,
  type ThreadNamed,
} from './thread-name';
export { displayBranch } from './display-branch';
export { pullRequestUrl } from './pull-request-url';
