import { http, HttpResponse, type RequestHandler } from 'msw';
import type {
  CreateLaunchOptionRequest,
  LaunchOption,
  LaunchOptionsResponse,
  UpdateLaunchOptionRequest,
} from '@delta/wire-gen';
import { launchOptionChoiceGroup } from './launchOptionChoiceGroup';
import { isDangerousLaunchOption } from './launchOptionDanger';
import type { HandlerContext } from './handlerContext';

/**
 * The refusal both launch-option write paths answer when a dangerous option is
 * asked to be default-enabled — one function here as on the real server, so the
 * two cannot state the same rule differently. The `code` is the stable one, so
 * the add form's copy path is drivable.
 */
function dangerousDefaultRejected(name: string) {
  return HttpResponse.json(
    {
      error: `\`${name}\` turns off the agent's own safety mechanism, so it cannot be enabled by default`,
      code: 'launch_option_rejected',
    },
    { status: 400 },
  );
}

/**
 * The row of `option`'s choice group, other than `option` itself, that already
 * holds the group's default — the mock's copy of the server's one-default rule.
 */
function choiceGroupDefaultHolder(
  options: LaunchOption[],
  option: Pick<LaunchOption, 'id' | 'provider' | 'choice_group'>,
): LaunchOption | undefined {
  if (option.choice_group === null) {
    return undefined;
  }
  return options.find(
    (other) =>
      other.id !== option.id &&
      other.provider === option.provider &&
      other.choice_group === option.choice_group &&
      other.default_enabled,
  );
}

/**
 * The refusal both launch-option write paths answer when a second row of one
 * choice group is asked to be default-enabled, naming the row that holds it.
 */
function secondDefaultRejected(group: string, holder: LaunchOption) {
  const described =
    holder.label ??
    (holder.value === null ? holder.name : `${holder.name} ${holder.value}`);
  return HttpResponse.json(
    {
      error: `\`${group}\` takes a single value and "${described}" is already enabled by default. Clear that default first, or leave this one off by default`,
      code: 'launch_option_rejected',
    },
    { status: 400 },
  );
}

/** The launch-option routes. */
export function launchOptionHandlers({
  store,
}: HandlerContext): RequestHandler[] {
  return [
    // The launch-option registry for the settings screen: list, create, delete.
    // Backed by the shared in-memory store so a created option lists and a
    // deleted one disappears, mirroring the real server's SQLite-backed CRUD.
    // The rows Delta ships are seeded like any other, so the mock exercises the
    // same badge / no-delete / 409 paths the real server produces.
    http.get('*/api/launch-options', () => {
      // The server's order: the rows Delta ships first (ascending id, i.e.
      // declared-catalog order), then the user's own newest first (descending
      // id).
      const launch_options = [...store.launchOptions].sort((a, b) =>
        a.builtin !== b.builtin
          ? Number(b.builtin) - Number(a.builtin)
          : a.builtin
            ? a.id - b.id
            : b.id - a.id,
      );
      const body: LaunchOptionsResponse = { launch_options };
      return HttpResponse.json(body);
    }),

    http.post('*/api/launch-options', async ({ request }) => {
      const payload = (await request.json()) as CreateLaunchOptionRequest;
      const name = typeof payload?.name === 'string' ? payload.name.trim() : '';
      if (name.length === 0) {
        // A blank name is a 400, exactly as the real server rejects it.
        return HttpResponse.json(
          { error: 'a launch option must have a non-blank `name`' },
          { status: 400 },
        );
      }
      const trimmedLabel = payload.label?.trim();
      const trimmedValue = payload.value?.trim();
      // Omitted `provider` defaults to Claude, matching the real server's
      // back-compat behavior.
      const provider = payload.provider ?? 'claude';
      const value = trimmedValue ? trimmedValue : null;
      const dangerous = isDangerousLaunchOption(provider, name, value);
      if (dangerous && payload.default_enabled === true) {
        // An option that disables the agent's own safety mechanism may be
        // registered but never pre-checked, exactly as the real server refuses
        // it.
        return dangerousDefaultRejected(name);
      }
      const choice_group = launchOptionChoiceGroup(provider, name);
      if (payload.default_enabled === true && choice_group !== null) {
        const holder = choiceGroupDefaultHolder(store.launchOptions, {
          id: -1,
          provider,
          choice_group,
        });
        if (holder) {
          return secondDefaultRejected(choice_group, holder);
        }
      }
      const option: LaunchOption = {
        id: store.nextLaunchOptionId++,
        label: trimmedLabel ? trimmedLabel : null,
        name,
        value,
        default_enabled: payload.default_enabled === true,
        created_at: new Date().toISOString(),
        provider,
        // Anything registered through the API is the user's own; only Delta's
        // startup reconcile writes a shipped row.
        builtin: false,
        dangerous,
        choice_group,
      };
      store.launchOptions.push(option);
      return HttpResponse.json(option, { status: 201 });
    }),

    http.patch('*/api/launch-options/:id', async ({ params, request }) => {
      const id = Number(params.id);
      const payload = (await request.json()) as UpdateLaunchOptionRequest;
      const option = store.launchOptions.find((o) => o.id === id);
      if (!option) {
        // An unknown id is a 404, exactly as the real server reports it.
        return HttpResponse.json(
          { error: `no launch option with id ${id}` },
          { status: 404 },
        );
      }
      if (option.dangerous && payload.default_enabled === true) {
        // The same refusal the create path gives; disabling stays allowed, so a
        // row that predates the rule can still be disarmed.
        return dangerousDefaultRejected(option.name);
      }
      if (payload.default_enabled === true) {
        const holder = choiceGroupDefaultHolder(store.launchOptions, option);
        if (holder && option.choice_group !== null) {
          // The server never flips a sibling itself: switching a group's
          // default is clear-then-set, so the reverse order is refused.
          return secondDefaultRejected(option.choice_group, holder);
        }
      }
      option.default_enabled = payload.default_enabled === true;
      return HttpResponse.json(option);
    }),

    http.delete('*/api/launch-options/:id', ({ params }) => {
      const id = Number(params.id);
      const target = store.launchOptions.find((o) => o.id === id);
      if (target?.builtin) {
        // A row Delta ships is not the user's to remove: a 409, and the row
        // stays. The Settings UI omits the control entirely, so this only
        // answers a stale list — but it has to answer it the way the real
        // server does, or that path could never be driven without a backend.
        return HttpResponse.json(
          {
            error: `launch option ${id} is built in and cannot be deleted`,
            code: 'launch_option_builtin',
          },
          { status: 409 },
        );
      }
      // Deleting an unknown id is a no-op (idempotent), like the real server.
      store.launchOptions = store.launchOptions.filter((o) => o.id !== id);
      return new HttpResponse(null, { status: 204 });
    }),
  ];
}
