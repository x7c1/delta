import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type {
  EnsureSessionResponse,
  MessagesResponse,
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

/** Spawn a brand-new session (`POST /api/sessions`); refresh the session list. */
export function useNewSessionMutation(
  client: ApiClient,
): UseMutationResult<EnsureSessionResponse, Error, void> {
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
    onSuccess: () => {
      // A branch send creates a thread server-side, and a new-session send
      // eventually adds a session; refresh the session list so the tree picks
      // up the change once the affected session's threads are refetched.
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
    },
  });
}
