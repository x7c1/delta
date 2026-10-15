import { Collapsible } from '@delta/ui-kit';
import type { FileChangeDetail, FileChangeKind } from '@delta/wire-gen';
import type { PermissionNotice as Notice } from '../../store/liveStore';
import { escapeInvisible, hasInvisible, toolArguments } from './permissionText';
import { VerbatimText } from './VerbatimText';

/**
 * Every argument of the request's tool input, each under its field name, in a
 * block that shows the value as it is (see {@link VerbatimText}).
 *
 * All of them, not a chosen one: which field decides the answer differs from
 * tool to tool — the `command` of a shell call, the `cwd` it runs in, the
 * `replace_all` of an edit — and a field the card left out is one the user
 * approved without seeing. The fields that say what runs and where come first.
 *
 * The list scrolls inside the card past a height, so a long value never
 * pushes the Allow and Deny buttons out of reach.
 */
export function PermissionArguments({ toolInput }: { toolInput: string }) {
  const args = toolArguments(toolInput);
  return (
    <dl
      className="flex max-h-[50vh] flex-col gap-1.5 overflow-y-auto"
      data-testid="permission-notice-arguments"
    >
      {args.length === 0 && (
        <dd className="text-caption text-fg-subtle">No arguments.</dd>
      )}
      {args.map((arg, index) => (
        <div key={`${arg.label ?? ''}-${index}`} className="flex flex-col">
          {arg.label !== null && (
            <dt className="break-all font-mono text-caption text-fg-subtle">
              {escapeInvisible(arg.label)}
            </dt>
          )}
          <dd>
            <VerbatimText
              text={arg.value}
              data-testid={`permission-notice-argument-${arg.label ?? 'input'}`}
            />
          </dd>
        </div>
      ))}
    </dl>
  );
}

/**
 * The human word for each change kind. A kind the server could not classify
 * arrives as `null` and simply gets no label — the path and diff are what the
 * answer turns on, and inventing a word for an unmodeled kind would be worse
 * than saying nothing.
 */
const CHANGE_KIND_LABEL: Record<FileChangeKind, string> = {
  add: 'new file',
  update: 'edit',
  delete: 'delete',
};

/**
 * What a file-change request would do on disk: the affected paths and their
 * change kinds always visible, the provider's stated reason when it gave one,
 * and the diff behind an expand control.
 *
 * The diff is deliberately NOT inline. A single approval can carry hundreds of
 * diff lines, and the question the user is answering — "should this agent write
 * to these files?" — is usually settled by the paths alone; burying the
 * Allow/Deny buttons under a wall of patch text would make the common answer
 * harder to give, not easier. Paths and diffs go through the same rendering
 * as the arguments, so an invisible character in either is shown.
 */
export function FileChangeSummary({ detail }: { detail: FileChangeDetail }) {
  const diffs = detail.changes.filter((change) => change.diff.length > 0);
  return (
    <div
      className="flex flex-col gap-1.5"
      data-testid="permission-notice-file-change"
    >
      <ul className="flex flex-col gap-0.5">
        {detail.changes.map((change, index) => (
          <li
            key={`${change.path}-${index}`}
            className="flex items-baseline gap-1.5"
          >
            <span className="break-all font-mono text-code text-fg">
              {escapeInvisible(change.path)}
            </span>
            {change.kind !== null && (
              <span className="shrink-0 text-caption text-fg-muted">
                {CHANGE_KIND_LABEL[change.kind]}
              </span>
            )}
          </li>
        ))}
      </ul>
      {detail.reason !== null && (
        <p className="text-fg-muted" data-testid="permission-notice-reason">
          {escapeInvisible(detail.reason)}
        </p>
      )}
      {diffs.length > 0 && (
        <Collapsible
          summary={`Diff (${diffs.length} ${diffs.length === 1 ? 'file' : 'files'})`}
        >
          {diffs.map((change, index) => (
            <div key={`${change.path}-${index}`} className="mb-1.5 last:mb-0">
              <div className="break-all font-mono text-code text-fg-subtle">
                {escapeInvisible(change.path)}
              </div>
              <VerbatimText text={change.diff} />
            </div>
          ))}
        </Collapsible>
      )}
    </div>
  );
}

/** Every string a value holds, keys included, at any depth. */
function stringsOf(value: unknown): string[] {
  if (typeof value === 'string') {
    return [value];
  }
  if (Array.isArray(value)) {
    return value.flatMap(stringsOf);
  }
  if (typeof value === 'object' && value !== null) {
    return Object.entries(value).flatMap(([key, inner]) => [
      key,
      ...stringsOf(inner),
    ]);
  }
  return [];
}

/**
 * Whether anything the card shows for `notice` holds an invisible or
 * direction-changing character: the tool name (a Codex command is used as
 * one), the input's keys and values (parsed, since JSON text may spell a
 * control character as an escape), the file-change paths, diffs and reason,
 * and the write root.
 */
export function requestHasInvisible(notice: Notice): boolean {
  let input: unknown;
  try {
    input = JSON.parse(notice.toolInput);
  } catch {
    input = notice.toolInput;
  }
  const texts = [
    notice.toolName,
    notice.toolInput,
    ...stringsOf(input),
    ...(notice.fileChange?.changes.flatMap((change) => [
      change.path,
      change.diff,
    ]) ?? []),
    notice.fileChange?.reason ?? '',
    notice.grantRoot ?? '',
  ];
  return texts.some(hasInvisible);
}
