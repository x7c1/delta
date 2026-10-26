import { describe, expect, it } from 'vitest';
import type { SessionListItem } from '@delta/wire-gen';
import { heldOrder } from './heldOrder';

function item(id: string, title: string | null = null): SessionListItem {
  return {
    session: {
      id,
      cwd: `/home/dev/${id}`,
      transcript_path: '',
      title,
      status: 'active',
      created_at: '2026-01-01T00:00:00Z',
      branch_at_launch: 'main',
      repo_root: `/home/dev/${id}`,
      repository_display_name: `dev/${id}`,
      provider: 'claude',
      provider_session_id: null,
      provider_thread_id: null,
      pull_request_number: null,
      failure_reason: null,
      closed_reason: null,
    },
    open: true,
    pane_starting: false,
    hooks_unreachable: false,
    turn: { state: 'idle', send_id: null, thread_id: null },
    running_subagents: [],
    main_thread_id: 1,
    threads: [
      {
        id: 1,
        session_id: id,
        title: 'main',
        parent_thread_id: null,
        root_message_uuid: null,
        created_at: '2026-01-01T00:00:00Z',
        last_activity_at: null,
      },
    ],
    last_activity_at: null,
  };
}

const ids = (items: SessionListItem[]) => items.map((i) => i.session.id);

describe('heldOrder', () => {
  it('keeps the held order when the live order has moved', () => {
    const live = [item('c'), item('a'), item('b')];

    expect(ids(heldOrder(live, ['a', 'b', 'c']))).toEqual(['a', 'b', 'c']);
  });

  it('shows the latest content of a held card', () => {
    const live = [item('b'), item('a', 'renamed')];

    const ordered = heldOrder(live, ['a', 'b']);

    expect(ordered[0]).toBe(live[1]);
    expect(ordered[0].session.title).toBe('renamed');
  });

  it('puts a session listed since the hold below the held ones, in the live order', () => {
    const live = [item('new-1'), item('b'), item('new-2'), item('a')];

    expect(ids(heldOrder(live, ['a', 'b']))).toEqual([
      'a',
      'b',
      'new-1',
      'new-2',
    ]);
  });

  it('drops a held session that is no longer listed', () => {
    const live = [item('c'), item('a')];

    expect(ids(heldOrder(live, ['a', 'b', 'c']))).toEqual(['a', 'c']);
  });

  it('places a held session the page walk listed twice once', () => {
    const live = [item('a'), item('b'), item('a')];

    expect(ids(heldOrder(live, ['b', 'a']))).toEqual(['b', 'a']);
  });
});
