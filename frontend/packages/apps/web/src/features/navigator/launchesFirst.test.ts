import { describe, expect, it } from 'vitest';
import type { SessionListItem } from '@delta/wire-gen';
import { launchesFirst } from './launchesFirst';

function item(
  id: string,
  status: 'spawning' | 'active' | 'ended' | 'failed',
): SessionListItem {
  const open = status === 'spawning' || status === 'active';
  return {
    session: {
      id,
      cwd: `/home/dev/${id}`,
      transcript_path: '',
      title: null,
      status,
      created_at: '2026-01-01T00:00:00Z',
      branch_at_launch: 'main',
      repo_root: `/home/dev/${id}`,
      repository_display_name: `dev/${id}`,
      provider: 'claude',
      provider_session_id: null,
      provider_thread_id: null,
      pull_request_number: null,
    },
    open,
    pane_starting: false,
    hooks_unreachable: false,
    main_thread_id: 1,
    last_activity_at: null,
  };
}

const ids = (items: SessionListItem[]) => items.map((i) => i.session.id);

describe('launchesFirst', () => {
  const sessions = [
    item('open-a', 'active'),
    item('starting', 'spawning'),
    item('closed-c', 'ended'),
    item('failed', 'failed'),
    item('closed-d', 'ended'),
  ];

  it('returns the server order untouched when nothing is launched', () => {
    expect(launchesFirst(sessions, [])).toBe(sessions);
  });

  it('pins a launch to the top and keeps the rest in the server order', () => {
    expect(ids(launchesFirst(sessions, ['failed']))).toEqual([
      'failed',
      'open-a',
      'starting',
      'closed-c',
      'closed-d',
    ]);
  });

  it('pins several launches in the order given', () => {
    expect(ids(launchesFirst(sessions, ['failed', 'starting']))).toEqual([
      'failed',
      'starting',
      'open-a',
      'closed-c',
      'closed-d',
    ]);
  });

  it('skips a launch whose session is not loaded', () => {
    expect(launchesFirst(sessions, ['elsewhere'])).toBe(sessions);
    expect(ids(launchesFirst(sessions, ['elsewhere', 'failed']))).toEqual([
      'failed',
      'open-a',
      'starting',
      'closed-c',
      'closed-d',
    ]);
  });

  it('leaves a tracked launch whose row is already active in the server order', () => {
    // A `session_registered` missed across a live-stream reconnect leaves the
    // spawn tracked, but the resync refetch lists its row as active.
    expect(launchesFirst(sessions, ['open-a'])).toBe(sessions);
    expect(ids(launchesFirst(sessions, ['open-a', 'failed']))).toEqual([
      'failed',
      'open-a',
      'starting',
      'closed-c',
      'closed-d',
    ]);
  });

  it('pins a session the page walk listed twice only once', () => {
    const twice = [...sessions, item('failed', 'failed')];
    expect(ids(launchesFirst(twice, ['failed']))).toEqual([
      'failed',
      'open-a',
      'starting',
      'closed-c',
      'closed-d',
    ]);
  });
});
