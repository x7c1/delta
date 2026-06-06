import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type {
  Message,
  MessagesResponse,
  SendRequest,
  SendResponse,
  SessionResponse,
  ThreadId,
  ThreadsResponse,
} from '@delta/model';
import type { ApiClient } from './http';

/**
 * TanStack Query integration for the REST surface. Query keys are centralised
 * here so the WebSocket layer can patch the same cache entries via
 * {@link appendMessage} / {@link invalidateThreads}.
 */
export const queryKeys = {
  session: ['session'] as const,
  threads: ['threads'] as const,
  messages: (threadId: ThreadId) => ['messages', threadId] as const,
};

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
    queryKey: threadId === null ? ['messages', 'none'] : queryKeys.messages(threadId),
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

// --- cache patching (driven by WebSocket events) ---------------------------

/**
 * Append a message to a thread's cached transcript, de-duplicating by uuid and
 * keeping the list ordered by `seq`. Used to apply incremental transcript
 * growth that arrives via the live channel. No-op if the thread is not cached.
 */
export function appendMessage(
  queryClient: QueryClient,
  threadId: ThreadId,
  message: Message,
): void {
  queryClient.setQueryData<MessagesResponse>(
    queryKeys.messages(threadId),
    (previous) => {
      if (!previous) {
        return previous;
      }
      const withoutDup = previous.messages.filter(
        (existing) => existing.uuid !== message.uuid,
      );
      const messages = [...withoutDup, message].sort((a, b) => a.seq - b.seq);
      return { messages };
    },
  );
}

/** Mark the session/threads queries stale so they refetch from the server. */
export function invalidateThreads(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: queryKeys.threads });
}

/** Mark a single thread's transcript stale so it refetches. */
export function invalidateThreadMessages(
  queryClient: QueryClient,
  threadId: ThreadId,
): void {
  void queryClient.invalidateQueries({
    queryKey: queryKeys.messages(threadId),
  });
}
