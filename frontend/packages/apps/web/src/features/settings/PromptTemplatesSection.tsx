import { useState, type FormEvent } from 'react';
import {
  useCreatePromptTemplateMutation,
  useDeletePromptTemplateMutation,
  usePromptTemplatesQuery,
  useUpdatePromptTemplateMutation,
} from '@delta/api-client';
import type { PromptTemplate } from '@delta/wire-gen';
import { Button, Dialog, Spinner } from '@delta/ui-kit';
import { useApiClient } from '../../data/apiContext';
import { settingsMutationErrorMessage } from './settingsMutationErrorMessage';

/**
 * The prompt-template editor's working copy, or `null` while the list view is
 * showing — the single piece of state that decides which of the category's two
 * views the right pane renders.
 *
 * `id` is `null` for a brand-new template (the save is a `POST`) and the
 * template's id when editing an existing one (a `PATCH`). The content fields
 * start empty or pre-filled accordingly.
 *
 * Deliberately local component state rather than anything persisted: the
 * section unmounts when its category is left or the dialog closes, which is
 * exactly the documented v1 behaviour — an abandoned draft is discarded
 * without a confirmation.
 */
interface PromptTemplateDraft {
  /** The template being edited, or `null` for a new one. */
  id: number | null;
  label: string;
  text: string;
}

/**
 * Prompt templates category content: manage the registry of named, reusable
 * blocks of instruction text. Unlike launch options, templates are global —
 * the body is prose the agent reads, so it means the same on every provider —
 * and there is no provider selector scoping the list.
 *
 * The pane shows one of two views at a time, never both: the **list** (a "New
 * template" button plus one row per registered template) or the **editor**
 * (label + text, reached from "New template" or a row's "Edit"). A template's
 * body is expected to run to many lines, so the list shows the label only —
 * no preview, no truncated first line — and the editor hands the textarea the
 * whole remaining pane height, scrolling internally rather than growing the
 * dialog.
 *
 * Deleting asks for confirmation first, which the launch-option rows
 * deliberately do not: a long template represents real writing, and there is
 * no undo.
 *
 * `active` mirrors the dialog's `settingsOpen` AND the category being the
 * visible one, so the query only runs while this section is mounted in the
 * right pane.
 */
export function PromptTemplatesSection({ active }: { active: boolean }) {
  const client = useApiClient();
  const templatesQuery = usePromptTemplatesQuery(client, active);
  const createTemplate = useCreatePromptTemplateMutation(client);
  const updateTemplate = useUpdatePromptTemplateMutation(client);
  const deleteTemplate = useDeletePromptTemplateMutation(client);

  const [draft, setDraft] = useState<PromptTemplateDraft | null>(null);
  // The template a delete has been requested for, i.e. the confirmation
  // dialog's subject; `null` while no confirmation is pending.
  const [pendingDelete, setPendingDelete] = useState<PromptTemplate | null>(
    null,
  );

  // React Query hands back a fresh mutation object every render, so pull the
  // stable `reset` functions out before using them in callbacks (the same
  // reason CloneRootsSection does it for its effect).
  const resetCreate = createTemplate.reset;
  const resetUpdate = updateTemplate.reset;
  const resetDelete = deleteTemplate.reset;

  /** Open the editor on a draft, clearing any error left by an earlier save. */
  const openEditor = (next: PromptTemplateDraft) => {
    resetCreate();
    resetUpdate();
    setDraft(next);
  };

  /** Return to the list, discarding the draft and any save error with it. */
  const closeEditor = () => {
    resetCreate();
    resetUpdate();
    setDraft(null);
  };

  const templates = templatesQuery.data?.prompt_templates ?? [];
  const saving = createTemplate.isPending || updateTemplate.isPending;
  // Both fields are required and the server rejects a blank one, so gate the
  // button on the trimmed values — the trim decides only whether saving is
  // allowed, never what is sent (see below).
  const canSave =
    draft !== null &&
    draft.label.trim().length > 0 &&
    draft.text.trim().length > 0 &&
    !saving;
  const saveError = createTemplate.error ?? updateTemplate.error;

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (draft === null || !canSave) {
      return;
    }
    // `text` goes over the wire verbatim: the server stores it byte-for-byte
    // and the composer inserts it the same way, so trimming here would
    // silently rewrite a template whose body deliberately opens or closes
    // with blank lines. The label is a one-line name, so stray edge
    // whitespace there is noise and is trimmed, as it is for launch options.
    const body = { label: draft.label.trim(), text: draft.text };
    const onSuccess = () => setDraft(null);
    if (draft.id === null) {
      createTemplate.mutate(body, { onSuccess });
    } else {
      updateTemplate.mutate({ id: draft.id, body }, { onSuccess });
    }
  };

  const confirmDelete = () => {
    if (pendingDelete === null) {
      return;
    }
    deleteTemplate.mutate(pendingDelete.id, {
      onSuccess: () => setPendingDelete(null),
    });
  };

  return (
    // A column that fills the pane: the header is fixed and the view below it
    // takes the rest, which is what lets the editor's textarea occupy the full
    // remaining height instead of a fixed number of rows.
    <section
      className="flex h-full w-full flex-col"
      data-testid="prompt-templates-section"
    >
      <h3 className="mb-1 text-secondary font-semibold text-fg">
        Prompt templates
      </h3>
      <p className="mb-4 text-caption text-fg-muted">
        Register the instruction text you write often — a review checklist, a
        merge routine — so it is written once and kept here instead of
        retyped. Templates are provider-independent: the same text reads the
        same on every agent.
      </p>

      {draft === null ? (
        <>
          <div className="mb-3 flex shrink-0 justify-end">
            <Button
              size="sm"
              variant="primary"
              onClick={() => openEditor({ id: null, label: '', text: '' })}
              data-testid="prompt-template-new"
            >
              New template
            </Button>
          </div>
          {/* The list scrolls within the pane rather than growing it, so the
              "New template" button stays put no matter how many templates
              are registered. */}
          <div className="min-h-0 flex-1 overflow-y-auto scrollbar-hover">
            {templatesQuery.isPending ? (
              <div className="flex justify-center py-6">
                <Spinner label="loading prompt templates" />
              </div>
            ) : templatesQuery.isError ? (
              <div className="flex flex-col items-center gap-2 py-6 text-secondary text-fg-muted">
                <p>Could not load prompt templates.</p>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => templatesQuery.refetch()}
                >
                  Retry
                </Button>
              </div>
            ) : templates.length === 0 ? (
              <p
                className="py-6 text-center text-secondary text-fg-subtle"
                data-testid="prompt-templates-empty"
              >
                No prompt templates yet.
              </p>
            ) : (
              <ul
                className="flex flex-col gap-2"
                data-testid="prompt-templates-list"
              >
                {templates.map((template) => (
                  <PromptTemplateRow
                    key={template.id}
                    template={template}
                    onEdit={() =>
                      openEditor({
                        id: template.id,
                        label: template.label,
                        text: template.text,
                      })
                    }
                    onDelete={() => {
                      resetDelete();
                      setPendingDelete(template);
                    }}
                  />
                ))}
              </ul>
            )}
          </div>
        </>
      ) : (
        <form
          onSubmit={onSubmit}
          className="flex min-h-0 flex-1 flex-col gap-3"
          aria-label={
            draft.id === null ? 'New prompt template' : 'Edit prompt template'
          }
          data-testid="prompt-template-editor"
        >
          <div className="flex shrink-0 flex-col gap-1">
            <label
              className="text-caption font-medium text-fg-muted"
              htmlFor="pt-label"
            >
              Label
            </label>
            <input
              id="pt-label"
              type="text"
              value={draft.label}
              onChange={(event) =>
                setDraft({ ...draft, label: event.target.value })
              }
              placeholder="Review checklist"
              required
              className="rounded border border-border-default bg-surface px-2 py-1 text-secondary text-fg placeholder:text-fg-subtle focus:border-accent-hover focus:outline-none"
            />
          </div>
          <div className="flex min-h-0 flex-1 flex-col gap-1">
            <label
              className="text-caption font-medium text-fg-muted"
              htmlFor="pt-text"
            >
              Text
            </label>
            {/* `flex-1` + `min-h-0` hands the textarea whatever the pane has
                left, and `resize-none` keeps that the only thing sizing it —
                a drag handle here would fight the dialog's fixed frame. Long
                bodies scroll inside it. The body text style is the app's own,
                so a template reads in the editor the way it will read in the
                composer. */}
            <textarea
              id="pt-text"
              value={draft.text}
              onChange={(event) =>
                setDraft({ ...draft, text: event.target.value })
              }
              placeholder="Review the diff on this branch with a critic's eye…"
              required
              className="min-h-0 flex-1 resize-none rounded border border-border-default bg-surface px-2 py-1.5 text-body text-fg placeholder:text-fg-subtle focus:border-accent-hover focus:outline-none scrollbar-hover"
            />
          </div>
          {saveError !== null && (
            <p
              className="shrink-0 text-caption text-danger"
              role="alert"
              data-testid="prompt-template-save-error"
            >
              {settingsMutationErrorMessage(
                saveError,
                'Could not save the prompt template.',
              )}
            </p>
          )}
          <div className="flex shrink-0 justify-end gap-2">
            <Button variant="ghost" size="sm" onClick={closeEditor}>
              Cancel
            </Button>
            <Button
              type="submit"
              variant="primary"
              size="sm"
              disabled={!canSave}
            >
              Save
            </Button>
          </div>
        </form>
      )}

      <Dialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        title="Delete prompt template"
        footer={
          <>
            <Button variant="ghost" onClick={() => setPendingDelete(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={confirmDelete}
              disabled={deleteTemplate.isPending}
              data-testid="prompt-template-delete-confirm"
            >
              Delete
            </Button>
          </>
        }
      >
        <div className="space-y-3">
          <p className="text-secondary text-fg">
            Delete{' '}
            <span className="font-medium">{pendingDelete?.label}</span>? Its
            text is not recoverable.
          </p>
          {/* A failed delete leaves the confirmation open so the user can
              retry from where they are, with the reason next to the button
              that failed. */}
          {deleteTemplate.isError && (
            <p
              className="text-caption text-danger"
              role="alert"
              data-testid="prompt-template-delete-error"
            >
              {settingsMutationErrorMessage(
                deleteTemplate.error,
                'Could not delete the prompt template.',
              )}
            </p>
          )}
        </div>
      </Dialog>
    </section>
  );
}

interface PromptTemplateRowProps {
  template: PromptTemplate;
  onEdit: () => void;
  onDelete: () => void;
}

/**
 * One row of the prompt-template list: the label and the two actions, and
 * deliberately nothing of the body. A template is a multi-paragraph block, so
 * any preview would either truncate it into nonsense or swamp the list; the
 * label is the name the user chose for exactly this purpose.
 */
function PromptTemplateRow({
  template,
  onEdit,
  onDelete,
}: PromptTemplateRowProps) {
  return (
    <li className="flex items-center justify-between gap-3 rounded-lg border border-border-default px-3 py-2">
      <span className="truncate text-secondary text-fg" title={template.label}>
        {template.label}
      </span>
      <div className="flex shrink-0 items-center gap-1">
        <Button
          size="sm"
          variant="ghost"
          onClick={onEdit}
          aria-label={`Edit prompt template ${template.label}`}
        >
          Edit
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={onDelete}
          aria-label={`Delete prompt template ${template.label}`}
        >
          Delete
        </Button>
      </div>
    </li>
  );
}
