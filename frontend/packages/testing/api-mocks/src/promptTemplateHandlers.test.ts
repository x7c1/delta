import { describe, expect, it } from 'vitest';
import type { HttpHandler } from 'msw';
import type { PromptTemplate, PromptTemplatesResponse } from '@delta/wire-gen';
import { createHandlers } from './handlers';
import { runGet, runPost, runWithMethod } from './handlerTestRequests';

/** The registered prompt templates via the mock `GET /api/prompt-templates`. */
async function getPromptTemplates(
  handlers: HttpHandler[],
): Promise<PromptTemplate[]> {
  const response = await runGet(
    handlers,
    '/api/prompt-templates',
    'http://localhost/api/prompt-templates',
  );
  expect(response.status).toBe(200);
  const body = (await response.json()) as PromptTemplatesResponse;
  return body.prompt_templates;
}

/**
 * The prompt-template mock reproduces the real server's CRUD contract, so the
 * Settings editor and the composer's insert menu can be driven end to end with
 * no backend: oldest-first ordering, a verbatim body, the blank-field 400, an
 * in-place edit that re-stamps `updated_at`, and an idempotent delete.
 */
describe('prompt-template mock CRUD', () => {
  it('seeds two templates, one of them long and multi-line', async () => {
    const templates = await getPromptTemplates(
      createHandlers() as HttpHandler[],
    );

    expect(templates).toHaveLength(2);
    // Oldest first, as the server returns them.
    expect(templates.map((t) => t.id)).toEqual([1, 2]);
    // One seed is realistic list-rendering material rather than a one-liner:
    // several paragraphs and well past 200 characters.
    const long = templates[1];
    expect(long.text.length).toBeGreaterThan(200);
    expect(long.text.split('\n').length).toBeGreaterThan(3);
  });

  it('creates, lists, edits in place, and deletes a template', async () => {
    const handlers = createHandlers() as HttpHandler[];

    // The body deliberately ends with a newline: the mock stores it verbatim,
    // as the server does.
    const created = await runPost(
      handlers,
      '/api/prompt-templates',
      'http://localhost/api/prompt-templates',
      { label: 'Ship it', text: 'Merge, then update the plan doc.\n' },
    );
    expect(created.status).toBe(201);
    const template = (await created.json()) as PromptTemplate;
    expect(template.text).toBe('Merge, then update the plan doc.\n');
    expect(template.updated_at).toBe(template.created_at);

    expect(await getPromptTemplates(handlers)).toHaveLength(3);

    const updated = await runWithMethod(
      handlers,
      'PATCH',
      '/api/prompt-templates/:id',
      `http://localhost/api/prompt-templates/${template.id}`,
      { label: 'Ship it carefully', text: 'Merge once green.' },
    );
    expect(updated.status).toBe(200);
    const edited = (await updated.json()) as PromptTemplate;
    expect(edited.id).toBe(template.id);
    expect(edited.label).toBe('Ship it carefully');
    expect(edited.created_at).toBe(template.created_at);

    // The edit replaced the row rather than adding one.
    const listed = await getPromptTemplates(handlers);
    expect(listed).toHaveLength(3);
    expect(listed.find((t) => t.id === template.id)?.label).toBe(
      'Ship it carefully',
    );

    const deleted = await runWithMethod(
      handlers,
      'DELETE',
      '/api/prompt-templates/:id',
      `http://localhost/api/prompt-templates/${template.id}`,
    );
    expect(deleted.status).toBe(204);
    expect(await getPromptTemplates(handlers)).toHaveLength(2);

    // Deleting it again is an idempotent no-op, like the real server.
    const again = await runWithMethod(
      handlers,
      'DELETE',
      '/api/prompt-templates/:id',
      `http://localhost/api/prompt-templates/${template.id}`,
    );
    expect(again.status).toBe(204);
  });

  it.each([
    ['blank label', { label: '   ', text: 'body' }],
    ['blank text', { label: 'Label', text: '\n\t ' }],
  ])('rejects a create with a %s with 400', async (_label, body) => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runPost(
      handlers,
      '/api/prompt-templates',
      'http://localhost/api/prompt-templates',
      body,
    );
    expect(response.status).toBe(400);
    expect(await getPromptTemplates(handlers)).toHaveLength(2);
  });

  it('answers 404 when editing an unknown id', async () => {
    const handlers = createHandlers() as HttpHandler[];
    const response = await runWithMethod(
      handlers,
      'PATCH',
      '/api/prompt-templates/:id',
      'http://localhost/api/prompt-templates/9999',
      { label: 'Label', text: 'text' },
    );
    expect(response.status).toBe(404);
  });

  it('answers 400 rather than 404 when a blank edit names an unknown id', async () => {
    // The server validates in the use case, before the store is consulted, so
    // the blank field wins over the missing row. The mock must agree, or a test
    // written against it would expect the wrong status from the real server.
    const handlers = createHandlers() as HttpHandler[];
    const response = await runWithMethod(
      handlers,
      'PATCH',
      '/api/prompt-templates/:id',
      'http://localhost/api/prompt-templates/9999',
      { label: '   ', text: 'text' },
    );
    expect(response.status).toBe(400);
  });
});
