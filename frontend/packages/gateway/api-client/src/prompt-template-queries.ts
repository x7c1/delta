import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type {
  CreatePromptTemplateRequest,
  PromptTemplate,
  PromptTemplatesResponse,
  UpdatePromptTemplateRequest,
} from '@delta/wire-gen';
import type { ApiClient } from './http';
import { queryKeys } from './query-keys';

/**
 * The registered prompt templates (`GET /api/prompt-templates`), oldest first,
 * for the settings screen to manage and the composer to insert from. Gated by
 * `enabled` so it only fetches while a consumer is mounted; mutations invalidate
 * this key to refresh the list.
 */
export function usePromptTemplatesQuery(
  client: ApiClient,
  enabled: boolean,
): UseQueryResult<PromptTemplatesResponse> {
  return useQuery({
    queryKey: queryKeys.promptTemplates,
    queryFn: () => client.getPromptTemplates(),
    enabled,
  });
}

/**
 * Register a prompt template (`POST /api/prompt-templates`); refresh the list on
 * success so the new row appears.
 */
export function useCreatePromptTemplateMutation(
  client: ApiClient,
): UseMutationResult<PromptTemplate, Error, CreatePromptTemplateRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreatePromptTemplateRequest) =>
      client.createPromptTemplate(body),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.promptTemplates,
      });
    },
  });
}

/**
 * Replace a prompt template's content (`PATCH /api/prompt-templates/{id}`);
 * refresh the list on success so the edited text is reflected.
 */
export function useUpdatePromptTemplateMutation(
  client: ApiClient,
): UseMutationResult<
  PromptTemplate,
  Error,
  { id: number; body: UpdatePromptTemplateRequest }
> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      body,
    }: {
      id: number;
      body: UpdatePromptTemplateRequest;
    }) => client.updatePromptTemplate(id, body),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.promptTemplates,
      });
    },
  });
}

/**
 * Delete a prompt template (`DELETE /api/prompt-templates/{id}`); refresh the
 * list on success so the removed row disappears.
 */
export function useDeletePromptTemplateMutation(
  client: ApiClient,
): UseMutationResult<void, Error, number> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => client.deletePromptTemplate(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.promptTemplates,
      });
    },
  });
}
