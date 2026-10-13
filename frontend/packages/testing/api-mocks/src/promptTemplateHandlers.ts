import { http, HttpResponse, type RequestHandler } from 'msw';
import type {
  CreatePromptTemplateRequest,
  PromptTemplate,
  PromptTemplatesResponse,
  UpdatePromptTemplateRequest,
} from '@delta/wire-gen';
import type { HandlerContext } from './handlerContext';

/**
 * The server's prompt-template validation, mirrored: `label` and `text` are both
 * required and must be non-blank once trimmed. Returns the error message the
 * server would answer with, or `null` when the body is acceptable. Only the
 * check trims — the handlers store what was sent.
 */
function blankPromptTemplateField(body: {
  label?: unknown;
  text?: unknown;
}): string | null {
  if (typeof body?.label !== 'string' || body.label.trim().length === 0) {
    return 'a prompt template must have a non-blank `label`';
  }
  if (typeof body?.text !== 'string' || body.text.trim().length === 0) {
    return 'a prompt template must have non-blank `text`';
  }
  return null;
}

/** The prompt-template routes. */
export function promptTemplateHandlers({
  store,
}: HandlerContext): RequestHandler[] {
  return [
    // The prompt-template registry: list, create, update, delete. Backed by the
    // shared in-memory store so a created template lists, an edit is reflected
    // in place, and a deleted one disappears — the full CRUD a component test
    // or an e2e-fake spec needs without a backend. The registry is global: no
    // provider filtering, unlike launch options.
    http.get('*/api/prompt-templates', () => {
      // Oldest first (ascending created_at, then id), as the server returns them.
      const prompt_templates = [...store.promptTemplates].sort(
        (a, b) => a.created_at.localeCompare(b.created_at) || a.id - b.id,
      );
      const body: PromptTemplatesResponse = { prompt_templates };
      return HttpResponse.json(body);
    }),

    http.post('*/api/prompt-templates', async ({ request }) => {
      const payload = (await request.json()) as CreatePromptTemplateRequest;
      const invalid = blankPromptTemplateField(payload);
      if (invalid) {
        return HttpResponse.json({ error: invalid }, { status: 400 });
      }
      const now = new Date().toISOString();
      const template: PromptTemplate = {
        id: store.nextPromptTemplateId++,
        // Stored verbatim — only the blank check trims, exactly as the server
        // does, so a template that ends with a newline keeps it.
        label: payload.label,
        text: payload.text,
        created_at: now,
        updated_at: now,
      };
      store.promptTemplates.push(template);
      return HttpResponse.json(template, { status: 201 });
    }),

    http.patch('*/api/prompt-templates/:id', async ({ params, request }) => {
      const id = Number(params.id);
      const payload = (await request.json()) as UpdatePromptTemplateRequest;
      // Validation runs before the lookup, matching the server: the use case
      // rejects a blank field before the store is ever asked about the id, so a
      // blank edit of an id that no longer exists is a 400, not a 404.
      const invalid = blankPromptTemplateField(payload);
      if (invalid) {
        return HttpResponse.json({ error: invalid }, { status: 400 });
      }
      const template = store.promptTemplates.find((t) => t.id === id);
      if (!template) {
        // An unknown id is a 404, exactly as the real server reports it.
        return HttpResponse.json(
          { error: `no prompt template with id ${id}` },
          { status: 404 },
        );
      }
      template.label = payload.label;
      template.text = payload.text;
      // Re-stamped on every edit, leaving created_at (and the list position) alone.
      template.updated_at = new Date().toISOString();
      return HttpResponse.json(template);
    }),

    http.delete('*/api/prompt-templates/:id', ({ params }) => {
      const id = Number(params.id);
      // Deleting an unknown id is a no-op (idempotent), like the real server.
      store.promptTemplates = store.promptTemplates.filter((t) => t.id !== id);
      return new HttpResponse(null, { status: 204 });
    }),
  ];
}
