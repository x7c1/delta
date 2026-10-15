/**
 * The prompt templates category.
 *
 * Shared fixtures live in SettingsView.testkit.tsx.
 */
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { createHandlers } from '@delta/api-mocks';
import {
  SETTINGS_STORAGE_KEY,
  useSettingsStore,
} from '../../store/settingsStore';
import {
  findTemplateList,
  FIXTURE_BODY_MARKER,
  MULTILINE_TEXT,
  renderSettings,
  resetSettingsState,
  server,
  switchToCloneRoots,
  switchToPromptTemplates,
  unstubGlobals,
} from './SettingsView.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
// A fresh mock registry per test, not just a reset of the per-test overrides:
// `createHandlers()` closes over an in-memory store, so one shared instance
// would let a template (or launch option) created by one test leak into the
// next one's list and make the file order-dependent.
beforeEach(() => server.resetHandlers(...createHandlers()));
afterAll(() => server.close());

describe('SettingsView', () => {
  beforeEach(resetSettingsState);
  afterEach(unstubGlobals);

  describe('Prompt templates section', () => {
    it('renders only the prompt-templates pane and persists the selection', async () => {
      renderSettings();
      switchToPromptTemplates();

      expect(
        await screen.findByTestId('prompt-templates-section'),
      ).toBeInTheDocument();
      expect(screen.queryByTestId('launch-options-section')).toBeNull();
      expect(screen.queryByTestId('clone-roots-section')).toBeNull();
      expect(
        screen.getByTestId('settings-category-prompt-templates'),
      ).toHaveAttribute('aria-selected', 'true');
      expect(useSettingsStore.getState().activeCategory).toBe(
        'prompt-templates',
      );
      const parsed = JSON.parse(
        localStorage.getItem(SETTINGS_STORAGE_KEY) ?? '{}',
      );
      expect(parsed.state.activeCategory).toBe('prompt-templates');
    });

    it('lists one row per template, showing the label and none of the body', async () => {
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      // The API's own order is the list's order: oldest first, so the fixture
      // created on 2026-01-01 precedes the one created on 2026-01-02. The pane
      // must not re-sort (by label, by `updated_at`, or otherwise) — a
      // template's position is the one the user will learn to reach for.
      const rows = within(list).getAllByRole('listitem');
      expect(rows).toHaveLength(2);
      expect(rows[0]).toHaveTextContent('Merge when green');
      expect(rows[1]).toHaveTextContent('Review checklist');

      // A template body runs to paragraphs, so no part of either fixture's
      // text may reach the pane — not even a truncated first line.
      const section = screen.getByTestId('prompt-templates-section');
      expect(section.textContent).not.toContain(FIXTURE_BODY_MARKER);
      expect(section.textContent).not.toContain('Once CI is green');

      for (const label of ['Merge when green', 'Review checklist']) {
        expect(
          within(list).getByRole('button', {
            name: `Edit prompt template ${label}`,
          }),
        ).toBeInTheDocument();
        expect(
          within(list).getByRole('button', {
            name: `Delete prompt template ${label}`,
          }),
        ).toBeInTheDocument();
      }
    });

    it('shows the empty state when nothing is registered', async () => {
      server.use(
        http.get('*/api/prompt-templates', () =>
          HttpResponse.json({ prompt_templates: [] }),
        ),
      );
      renderSettings();
      switchToPromptTemplates();

      const empty = await screen.findByTestId('prompt-templates-empty');
      expect(empty.textContent).toBe('No prompt templates yet.');
      expect(screen.queryByTestId('prompt-templates-list')).toBeNull();
    });

    it('opens an empty editor from New template and gates Save on both fields', async () => {
      renderSettings();
      switchToPromptTemplates();
      await findTemplateList();

      fireEvent.click(screen.getByTestId('prompt-template-new'));

      // The editor replaces the list inside the same pane.
      expect(screen.getByTestId('prompt-template-editor')).toBeInTheDocument();
      expect(screen.queryByTestId('prompt-templates-list')).toBeNull();
      expect(screen.queryByTestId('prompt-template-new')).toBeNull();

      const label = screen.getByLabelText('Label');
      const text = screen.getByLabelText('Text');
      expect(label).toHaveValue('');
      expect(text).toHaveValue('');
      const save = screen.getByRole('button', { name: 'Save' });
      expect(save).toBeDisabled();

      // Whitespace is not content: the server rejects a blank field, so the
      // button must not promise otherwise.
      fireEvent.change(label, { target: { value: '   ' } });
      fireEvent.change(text, { target: { value: MULTILINE_TEXT } });
      expect(save).toBeDisabled();

      fireEvent.change(label, { target: { value: 'Triage' } });
      expect(save).toBeEnabled();
    });

    it('creates a template, sending the text verbatim, and returns to the list', async () => {
      const posted: unknown[] = [];
      server.use(
        http.post('*/api/prompt-templates', async ({ request }) => {
          posted.push(await request.clone().json());
          // Returning nothing hands the request to the default mock handler,
          // so the create still round-trips through the mock store and the
          // refreshed list shows the new row.
        }),
      );
      renderSettings();
      switchToPromptTemplates();
      await findTemplateList();

      fireEvent.click(screen.getByTestId('prompt-template-new'));
      fireEvent.change(screen.getByLabelText('Label'), {
        target: { value: 'Triage' },
      });
      fireEvent.change(screen.getByLabelText('Text'), {
        target: { value: MULTILINE_TEXT },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save' }));

      const list = await findTemplateList();
      await within(list).findByText('Triage');
      expect(screen.queryByTestId('prompt-template-editor')).toBeNull();
      // Newlines and blank lines survive: the body is never trimmed or
      // collapsed on its way to the server.
      expect(posted).toEqual([{ label: 'Triage', text: MULTILINE_TEXT }]);
    });

    it('edits a template from its row and patches the label and text', async () => {
      const patched: { id: string; body: unknown }[] = [];
      server.use(
        http.patch('*/api/prompt-templates/:id', async ({ params, request }) => {
          patched.push({
            id: String(params.id),
            body: await request.clone().json(),
          });
        }),
      );
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      fireEvent.click(
        within(list).getByRole('button', {
          name: 'Edit prompt template Review checklist',
        }),
      );

      // Pre-filled with the row's own content — the whole body, not a preview.
      expect(screen.getByLabelText('Label')).toHaveValue('Review checklist');
      const text = screen.getByLabelText('Text') as HTMLTextAreaElement;
      expect(text.value).toContain(FIXTURE_BODY_MARKER);
      expect(text.value.split('\n').length).toBeGreaterThan(1);
      const originalText = text.value;

      fireEvent.change(screen.getByLabelText('Label'), {
        target: { value: 'Review checklist v2' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save' }));

      // The list is re-queried: the editor unmounted the previous `ul`, so the
      // one showing now is a different node.
      const updated = await findTemplateList();
      await within(updated).findByText('Review checklist v2');
      expect(screen.queryByTestId('prompt-template-editor')).toBeNull();
      expect(patched).toEqual([
        {
          // The seeded `Review checklist` fixture.
          id: '2',
          body: { label: 'Review checklist v2', text: originalText },
        },
      ]);
    });

    it('cancels the editor without a request and without changing the list', async () => {
      const writes: string[] = [];
      server.use(
        http.post('*/api/prompt-templates', () => {
          writes.push('POST');
          return HttpResponse.json({ error: 'unexpected' }, { status: 500 });
        }),
        http.patch('*/api/prompt-templates/:id', () => {
          writes.push('PATCH');
          return HttpResponse.json({ error: 'unexpected' }, { status: 500 });
        }),
      );
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      fireEvent.click(
        within(list).getByRole('button', {
          name: 'Edit prompt template Merge when green',
        }),
      );
      fireEvent.change(screen.getByLabelText('Label'), {
        target: { value: 'Discarded' },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      const back = await findTemplateList();
      expect(screen.queryByTestId('prompt-template-editor')).toBeNull();
      expect(within(back).getByText('Merge when green')).toBeInTheDocument();
      expect(within(back).queryByText('Discarded')).toBeNull();
      expect(writes).toEqual([]);

      // Reopening starts from the stored content, so the abandoned edit is
      // genuinely gone rather than parked.
      fireEvent.click(
        within(back).getByRole('button', {
          name: 'Edit prompt template Merge when green',
        }),
      );
      expect(screen.getByLabelText('Label')).toHaveValue('Merge when green');
    });

    it('discards an open draft when the category is left', async () => {
      renderSettings();
      switchToPromptTemplates();
      await findTemplateList();
      fireEvent.click(screen.getByTestId('prompt-template-new'));
      fireEvent.change(screen.getByLabelText('Label'), {
        target: { value: 'Half-written' },
      });

      switchToCloneRoots();
      await screen.findByTestId('clone-roots-section');
      switchToPromptTemplates();

      // Back on the list, not on the half-written draft.
      await findTemplateList();
      expect(screen.queryByTestId('prompt-template-editor')).toBeNull();
    });

    it('confirms a delete by name and removes the row', async () => {
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      fireEvent.click(
        within(list).getByRole('button', {
          name: 'Delete prompt template Merge when green',
        }),
      );

      // The confirmation names the template it is about to destroy.
      const confirmation = screen
        .getByText('Delete prompt template')
        .closest('[role="dialog"]');
      expect(confirmation).not.toBeNull();
      expect(confirmation?.textContent).toContain('Merge when green');

      fireEvent.click(screen.getByTestId('prompt-template-delete-confirm'));

      await waitFor(() =>
        expect(within(list).queryByText('Merge when green')).toBeNull(),
      );
      expect(within(list).getByText('Review checklist')).toBeInTheDocument();
      // The confirmation closes itself once the delete lands.
      expect(screen.queryByTestId('prompt-template-delete-confirm')).toBeNull();
    });

    it('issues no request when the delete confirmation is dismissed', async () => {
      const deletes: string[] = [];
      server.use(
        http.delete('*/api/prompt-templates/:id', ({ params }) => {
          deletes.push(String(params.id));
          return new HttpResponse(null, { status: 204 });
        }),
      );
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      fireEvent.click(
        within(list).getByRole('button', {
          name: 'Delete prompt template Merge when green',
        }),
      );
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      expect(screen.queryByTestId('prompt-template-delete-confirm')).toBeNull();
      expect(within(list).getByText('Merge when green')).toBeInTheDocument();
      expect(deletes).toEqual([]);
    });

    it('surfaces a failed save inline and keeps the editor content', async () => {
      server.use(
        http.post('*/api/prompt-templates', () =>
          HttpResponse.json(
            { error: 'a prompt template must have a non-blank `label`' },
            { status: 400 },
          ),
        ),
      );
      renderSettings();
      switchToPromptTemplates();
      await findTemplateList();

      fireEvent.click(screen.getByTestId('prompt-template-new'));
      fireEvent.change(screen.getByLabelText('Label'), {
        target: { value: 'Triage' },
      });
      fireEvent.change(screen.getByLabelText('Text'), {
        target: { value: MULTILINE_TEXT },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save' }));

      const error = await screen.findByTestId('prompt-template-save-error');
      expect(error.textContent).toContain('Could not save the prompt template.');
      // The server's own reason is repeated rather than swallowed.
      expect(error.textContent).toContain('non-blank');
      // The draft survives so the user can fix it and retry.
      expect(screen.getByTestId('prompt-template-editor')).toBeInTheDocument();
      expect(screen.getByLabelText('Label')).toHaveValue('Triage');
      expect(screen.getByLabelText('Text')).toHaveValue(MULTILINE_TEXT);
    });

    it('surfaces a failed delete and keeps the row', async () => {
      server.use(
        http.delete('*/api/prompt-templates/:id', () =>
          HttpResponse.json({ error: 'database is locked' }, { status: 500 }),
        ),
      );
      renderSettings();
      switchToPromptTemplates();
      const list = await findTemplateList();

      fireEvent.click(
        within(list).getByRole('button', {
          name: 'Delete prompt template Merge when green',
        }),
      );
      fireEvent.click(screen.getByTestId('prompt-template-delete-confirm'));

      const error = await screen.findByTestId('prompt-template-delete-error');
      expect(error.textContent).toContain(
        'Could not delete the prompt template.',
      );
      expect(error.textContent).toContain('database is locked');
      expect(within(list).getByText('Merge when green')).toBeInTheDocument();
    });
  });
});
