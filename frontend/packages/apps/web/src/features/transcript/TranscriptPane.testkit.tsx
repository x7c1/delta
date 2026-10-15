/**
 * Shared fixtures and helpers for the `TranscriptPane.*.test.tsx` family: the
 * MSW server, the render helpers, the per-case store reset, and the
 * controllable `ResizeObserver` the overlay-measurement cases drive.
 *
 * Not a test file itself: the `.testkit.` segment keeps it outside vitest's
 * `*.test.*` include glob.
 */
import type { ComponentProps } from 'react';
import { render, type RenderResult } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import {
  MAIN_THREAD_ID,
  SESSION_ID,
  createHandlers,
  mockThreads,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import { useComposerStore } from '../../store/composerStore';
import {
  TIMELINE_EXPANDED_SUBKEY,
  resetTimelineExpandedForTests,
} from './useTimelineExpanded';
import { sessionScopedKey } from '../../store/sessionScopedStorage';
import { TranscriptPane } from './TranscriptPane';

/**
 * Compose the localStorage key the timeline uses for the mock session every
 * test in this family points at. The TranscriptPane reads the focused session
 * id from `navStore`; cases that seed an expanded preference (or assert one)
 * also pin the focus to {@link SESSION_ID} so the hook persists against the
 * matching per-session key.
 */
export function timelineExpandedKey(sessionId: string = SESSION_ID): string {
  return sessionScopedKey(sessionId, TIMELINE_EXPANDED_SUBKEY);
}

// Per-message metadata the cases below do not depend on.
export const NO_METADATA = {
  model: null,
  git_branch: null,
  cwd: null,
  response_time_ms: null,
  provider_item_id: null,
} as const;

export const server = setupServer(...createHandlers());

export function renderPane(
  threads = mockThreads,
  activeThreadId = MAIN_THREAD_ID,
  props: Partial<ComponentProps<typeof TranscriptPane>> = {},
): RenderResult {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  const active = threads.find((t) => t.id === activeThreadId)!;
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <TranscriptPane
          threads={threads}
          activeThread={active}
          readOnly={false}
          {...props}
        />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

/** Reset the stores and the timeline preference every case starts from. */
export function resetTranscriptPaneState(): void {
  useNavStore.setState({
    activeThreadId: MAIN_THREAD_ID,
    activeThreadJumpTarget: null,
    focusedSessionId: NEW_SESSION_FOCUS,
    preNewSessionFocus: null,
  });
  useLiveStore.setState({
    sending: [],
    localSends: {},
    spawns: [],
    notices: {},
    resolvedRequests: {},
    streamingMessages: {},
    runningSubagents: {},
  });
  useComposerStore.setState({
    drafts: {},
    branchOrigin: null,
    newSessionWorkdir: null,
    workdirDialogOpen: false,
  });
  // Clear the timeline-expanded persisted preference and the in-memory
  // cache so each test starts from the default collapsed state (some
  // tests opt into expanded explicitly). The cache resets every per-session
  // entry when called without an id; clearing localStorage in lockstep
  // wipes any session-scoped keys an earlier case may have written.
  window.localStorage.clear();
  resetTimelineExpandedForTests();
}

export function renderNewSessionPane(
  threads = mockThreads,
  { workdirMandatory = false } = {},
): RenderResult {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <TranscriptPane
          threads={threads}
          activeThread={null}
          readOnly={false}
          newSession
          workdirMandatory={workdirMandatory}
        />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

// The body reserves bottom space equal to the bottom overlay's MEASURED
// height, so the composer growing pushes the conversation tail up instead of
// hiding it. jsdom performs no layout (every `getBoundingClientRect` is 0 and
// ResizeObserver never fires on its own), so we drive both explicitly: stub
// the overlay's measured height and a controllable ResizeObserver, then fire
// it to simulate the composer growing.

/**
 * Live observations keyed by the observed element. TranscriptPane creates
 * several ResizeObservers (body re-stick, bottom overlay, top region), so
 * the per-test code looks up exactly the observer it cares about by node
 * rather than reading the "most recent" one, which would race with effect
 * ordering.
 */
let observations: Map<Element, ResizeObserverCallback>;
let originalRO: typeof ResizeObserver;

export function lookupObserver(el: Element | null): ResizeObserverCallback | null {
  return el ? observations.get(el) ?? null : null;
}

/** Replace `ResizeObserver` with one the cases fire by hand. */
export function installControllableResizeObserver(): void {
  observations = new Map();
  originalRO = globalThis.ResizeObserver;
  class ControllableRO implements ResizeObserver {
    private observedEls = new Set<Element>();
    constructor(private cb: ResizeObserverCallback) {}
    observe(el: Element): void {
      observations.set(el, this.cb);
      this.observedEls.add(el);
    }
    unobserve(el: Element): void {
      observations.delete(el);
      this.observedEls.delete(el);
    }
    disconnect(): void {
      for (const el of this.observedEls) {
        observations.delete(el);
      }
      this.observedEls.clear();
    }
  }
  globalThis.ResizeObserver =
    ControllableRO as unknown as typeof ResizeObserver;
}

/** Put the real `ResizeObserver` back. */
export function restoreResizeObserver(): void {
  globalThis.ResizeObserver = originalRO;
}

/** The Panel scroll body (the element that carries the reserve padding). */
export function bodyEl(): HTMLElement {
  return document.querySelector('.scrollbar-hover') as HTMLElement;
}
