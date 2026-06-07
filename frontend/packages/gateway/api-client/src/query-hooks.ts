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
  SessionResponse,
  ThreadId,
  ThreadsResponse,
} from '@delta/model';
import type { ApiClient } from './http';
import { queryKeys } from './query-keys';

/**
 * Ensure the Claude Code session is up. Runs once on app load; the server starts
 * the session lazily if absent and reuses it if present. A bounded retry covers
 * a transient failure while the server is still coming up, but does not spin
 * forever — a persistent failure surfaces as an error the UI can show.
 */
export function useEnsureSessionQuery(
  client: ApiClient,
): UseQueryResult<EnsureSessionResponse> {
  return useQuery({
    queryKey: queryKeys.ensureSession,
    queryFn: () => client.ensureSession(),
    retry: 2,
    staleTime: Infinity,
    gcTime: Infinity,
  });
}

export function useSessionQuery(
  client: ApiClient,
): UseQueryResult<SessionResponse> {
  return useQuery({
    queryKey: queryKeys.session,
    queryFn: () => client.getSession(),
  });
}

export function useThreadsQuery(
  client: ApiClient,
): UseQueryResult<ThreadsResponse> {
  return useQuery({
    queryKey: queryKeys.threads,
    queryFn: () => client.getThreads(),
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

export function useCreateSendMutation(
  client: ApiClient,
): UseMutationResult<SendResponse, Error, SendRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: SendRequest) => client.createSend(body),
    onSuccess: () => {
      // A new branch send creates a thread server-side; refresh the tree.
      void queryClient.invalidateQueries({ queryKey: queryKeys.threads });
    },
  });
}
