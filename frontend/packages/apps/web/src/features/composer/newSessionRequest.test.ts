import { describe, expect, it } from 'vitest';
import type { NewSessionLaunch } from '../../store/liveStore';
import { newSessionSendBody } from './newSessionRequest';

/** A launch with only its directory chosen: the shape a bare New session send has. */
const bare: NewSessionLaunch = {
  workdir: '/projects/delta',
  launchOptionIds: [],
  provider: 'claude',
  worktree: null,
  pullRequestNumber: null,
};

describe('newSessionSendBody', () => {
  it('omits every optional field when nothing but the directory was selected', () => {
    expect(newSessionSendBody('hi', bare)).toEqual({
      new_session: true,
      text: 'hi',
      workdir: '/projects/delta',
    });
  });

  it('attaches pull_request_number when the workdir came from a PR', () => {
    expect(
      newSessionSendBody('resume PR work', {
        ...bare,
        pullRequestNumber: 138,
      }),
    ).toEqual({
      new_session: true,
      text: 'resume PR work',
      workdir: '/projects/delta',
      pull_request_number: 138,
    });
  });

  it('omits pull_request_number entirely when there is no PR origin', () => {
    // Not `null` on the wire: an omitted field is what the server's serde
    // default reads as "no PR", and it keeps a non-PR send byte-identical to
    // the body sent before this field existed.
    const body = newSessionSendBody('hi', bare);
    expect(body).not.toHaveProperty('pull_request_number');
  });
});
