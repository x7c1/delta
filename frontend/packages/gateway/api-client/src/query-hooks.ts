import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type {
  MessagesResponse,
  NewSessionResponse,
  SendRequest,
  SendResponse,
  SessionId,
  SessionsResponse,
  ThreadId,
  ThreadsResponse,
} from '@delta/model';
import type { ApiClient } from './http';
import { queryKeys } from './query-keys';

/**
 * The session list (`GET /api/sessions`). Runs on app load and is invalidated by
 * lifecycle events (`session_registered`/`session_opened`/`session_closed`). A
 * bounded retry covers a transient failure while the server is still coming up.
 */
export function useSessionsQuery(
  client: ApiClient,
): UseQueryResult<SessionsResponse> {
  return useQuery({
    queryKey: queryKeys.sessions,
    queryFn: () => client.getSessions(),
    retry: 2,
  });
}

/** A single session's thread tree. Disabled until a session is focused. */
export function useSessionThreadsQuery(
  client: ApiClient,
  sessionId: SessionId | null,
): UseQueryResult<ThreadsResponse> {
  return useQuery({
    queryKey:
      sessionId === null
        ? queryKeys.sessionThreadsNone
        : queryKeys.sessionThreads(sessionId),
    queryFn: () => client.getSessionThreads(sessionId as SessionId),
    enabled: sessionId !== null,
  });
}

export function useThreadMessagesQuery(
  client: ApiClient,
  threadId: ThreadId | null,
): UseQueryResult<MessagesResponse> {
  return useQuery({
    queryKey:
      threadId === null ? queryKeys.messagesNone : queryKeys.messages(threadId),
    queryFn: () => client.getThreadMessages(threadId as ThreadId),
    enabled: threadId !== null,
  });
}

/**
 * Spawn a brand-new session (`POST /api/sessions`); refresh the session list.
 *
 * Library surface for an explicit "New session" affordance. The current UI does
 * not use it: New starts an empty composer and the first Send (`new_session:
 * true`) spawns the session, so the optimistic pending item reconciles in one
 * round-trip. Likewise `useOpenSessionMutation` exists for an explicit Resume,
 * but the UI resumes a closed session by sending to its main thread (the backend
 * auto-resumes). Only `useCloseSessionMutation` is currently wired (the
 * navigator Close button).
 */
export function useNewSessionMutation(
  client: ApiClient,
): UseMutationResult<NewSessionResponse, Error, void> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => client.newSession(),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}

/** Resume a closed session (`POST /api/sessions/{id}/open`). */
export function useOpenSessionMutation(
  client: ApiClient,
): UseMutationResult<void, Error, SessionId> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (sessionId: SessionId) => client.openSession(sessionId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}

/** Close an open session (`POST /api/sessions/{id}/close`). */
export function useCloseSessionMutation(
  client: ApiClient,
): UseMutationResult<void, Error, SessionId> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (sessionId: SessionId) => client.closeSession(sessionId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}

export function useCreateSendMutation(
  client: ApiClient,
): UseMutationResult<SendResponse, Error, SendRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: SendRequest) => client.createSend(body),
    onSuccess: ({ send }) => {
      // A branch send creates a child thread server-side, and a new-session
      // send eventually adds a session; refresh the session list so the tree
      // picks up the change.
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
      // Also refresh the affected session's thread tree so a freshly-branched
      // child appears immediately. Without this the new thread is absent from
      // the cached list, and the workspace reverts the active thread back to
      // main instead of drilling into the new branch. A new-session send has
      // no bound session yet (synthetic id 0 / empty session id), so it only
      // refreshes once the session registers via the list.
      if (send.session_id) {
        void queryClient.invalidateQueries({
          queryKey: queryKeys.sessionThreads(send.session_id),
        });
      }
    },
  });
}
