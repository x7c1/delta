import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import { createHandlers } from '@delta/api-mocks';
import {
  ApiClient,
  queryKeys,
  useCreatePromptTemplateMutation,
  useDeletePromptTemplateMutation,
  usePromptTemplatesQuery,
  useUpdatePromptTemplateMutation,
} from '@delta/api-client';
import type { PropsWithChildren } from 'react';

/**
 * The prompt-template hooks against the mock server: what a caller of
 * `@delta/api-client` actually gets. The point of these is the cache contract —
 * every mutation invalidates `queryKeys.promptTemplates`, so a list rendered
 * from `usePromptTemplatesQuery` reflects a create, an edit, or a delete without
 * the caller refetching by hand. The hooks themselves live in the gateway
 * package, which has no DOM test setup; this is where React hooks can be
 * rendered.
 */

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

/** A fresh client + provider per test, so no cache leaks between them. */
function harness(): {
  client: ApiClient;
  queryClient: QueryClient;
  wrapper: (props: PropsWithChildren) => JSX.Element;
} {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  const wrapper = ({ children }: PropsWithChildren) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { client, queryClient, wrapper };
}

describe('prompt-template query hooks', () => {
  it('lists the seeded templates, oldest first', async () => {
    const { client, wrapper } = harness();

    const { result } = renderHook(() => usePromptTemplatesQuery(client, true), {
      wrapper,
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    const templates = result.current.data!.prompt_templates;
    expect(templates.map((t) => t.label)).toEqual([
      'Merge when green',
      'Review checklist',
    ]);
  });

  it('does not fetch while disabled', () => {
    const { client, wrapper } = harness();

    const { result } = renderHook(() => usePromptTemplatesQuery(client, false), {
      wrapper,
    });

    expect(result.current.fetchStatus).toBe('idle');
    expect(result.current.data).toBeUndefined();
  });

  it('refreshes the list after a create, an update, and a delete', async () => {
    const { client, queryClient, wrapper } = harness();

    const { result } = renderHook(
      () => ({
        list: usePromptTemplatesQuery(client, true),
        create: useCreatePromptTemplateMutation(client),
        update: useUpdatePromptTemplateMutation(client),
        remove: useDeletePromptTemplateMutation(client),
      }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.list.isSuccess).toBe(true));
    expect(result.current.list.data!.prompt_templates).toHaveLength(2);

    // Create: the new row appears without the caller refetching.
    const created = await result.current.create.mutateAsync({
      label: 'Ship it',
      text: 'Merge, then update the plan doc.\n',
    });
    await waitFor(() =>
      expect(result.current.list.data!.prompt_templates).toHaveLength(3),
    );
    // The body survives the round trip byte for byte, trailing newline included.
    expect(
      result.current.list.data!.prompt_templates.find(
        (t) => t.id === created.id,
      )?.text,
    ).toBe('Merge, then update the plan doc.\n');

    // Update: the edited label is reflected in place.
    await result.current.update.mutateAsync({
      id: created.id,
      body: { label: 'Ship it carefully', text: 'Merge once green.' },
    });
    await waitFor(() =>
      expect(
        result.current.list.data!.prompt_templates.find(
          (t) => t.id === created.id,
        )?.label,
      ).toBe('Ship it carefully'),
    );

    // Delete: the row disappears.
    await result.current.remove.mutateAsync(created.id);
    await waitFor(() =>
      expect(result.current.list.data!.prompt_templates).toHaveLength(2),
    );

    // All three went through the one shared key, which is what lets an unrelated
    // consumer of the same list stay in sync.
    expect(
      queryClient.getQueryData(queryKeys.promptTemplates),
    ).not.toBeUndefined();
  });

  it('surfaces the server 400 for a blank template rather than mutating', async () => {
    const { client, wrapper } = harness();

    const { result } = renderHook(
      () => ({
        list: usePromptTemplatesQuery(client, true),
        create: useCreatePromptTemplateMutation(client),
      }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.list.isSuccess).toBe(true));

    await expect(
      result.current.create.mutateAsync({ label: '   ', text: 'body' }),
    ).rejects.toMatchObject({ status: 400 });
    expect(result.current.list.data!.prompt_templates).toHaveLength(2);
  });
});
