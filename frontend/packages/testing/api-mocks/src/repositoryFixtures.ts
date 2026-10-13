/** Mock pull requests for the new-session PR tab, shared by `reviewer`
 *  and `author` lens fixtures. The first PR is on a repo with a
 *  registered local clone (`x7c1/delta`, see `mockRepositories`); the
 *  second is on a repo with no local clone (`x7c1/other`) so the
 *  no-clone path — the inline clone panel — is exercisable alongside
 *  the happy path. */
export function mockReviewerPullRequests(): {
  number: number;
  title: string;
  repo_owner: string;
  repo_name: string;
  head_ref: string;
  head_repo_owner: string;
  head_repo_name: string;
  draft: boolean;
  url: string;
  updated_at: string;
  author_login: string;
  has_local_clone: boolean;
}[] {
  return [
    {
      number: 174,
      title: 'feat: add Repository tab to the new-session screen',
      repo_owner: 'x7c1',
      repo_name: 'delta',
      head_ref: 'feat/repo-tab',
      head_repo_owner: 'x7c1',
      head_repo_name: 'delta',
      draft: false,
      url: 'https://github.com/x7c1/delta/pull/174',
      updated_at: '2026-06-20T11:33:21Z',
      author_login: 'collaborator',
      has_local_clone: true,
    },
    {
      number: 9,
      title: 'fix: something obscure',
      repo_owner: 'x7c1',
      repo_name: 'other',
      head_ref: 'fix/obscure',
      head_repo_owner: 'x7c1',
      head_repo_name: 'other',
      draft: false,
      url: 'https://github.com/x7c1/other/pull/9',
      updated_at: '2026-06-19T08:00:00Z',
      author_login: 'collaborator',
      has_local_clone: false,
    },
  ];
}

export function mockAuthorPullRequests(): ReturnType<
  typeof mockReviewerPullRequests
> {
  return [
    {
      number: 200,
      title: 'wip: my own draft',
      repo_owner: 'x7c1',
      repo_name: 'delta',
      head_ref: 'feat/my-draft',
      head_repo_owner: 'x7c1',
      head_repo_name: 'delta',
      draft: true,
      url: 'https://github.com/x7c1/delta/pull/200',
      updated_at: '2026-06-24T01:00:00Z',
      author_login: 'x7c1',
      has_local_clone: true,
    },
  ];
}

/** Mock repositories for the new-session Repository tab. The default
 *  bundles two clones under one origin URL; the second is a single-clone
 *  entry whose `origin` was unset so it falls back to a path-keyed entry. */
export function mockRepositories(): {
  identity_key: string;
  display_name: string;
  recently_used_clone_path: string;
  clones: {
    path: string;
    last_opened_at: string | null;
    last_branch: string | null;
    last_launch_option_ids: number[];
    last_worktree_enabled: boolean;
    last_worktree_start_point: null;
  }[];
}[] {
  return [
    {
      identity_key: 'github.com/x7c1/delta',
      display_name: 'x7c1/delta',
      recently_used_clone_path: '/home/dev/projects/delta',
      clones: [
        {
          path: '/home/dev/projects/delta',
          last_opened_at: '2026-01-03T00:00:00Z',
          last_branch: 'main',
          last_launch_option_ids: [],
          last_worktree_enabled: false,
          last_worktree_start_point: null,
        },
        {
          path: '/home/dev/projects/delta-fork',
          last_opened_at: '2026-01-02T00:00:00Z',
          last_branch: 'feature/x',
          last_launch_option_ids: [],
          last_worktree_enabled: false,
          last_worktree_start_point: null,
        },
      ],
    },
    {
      identity_key: '/home/dev/projects/website',
      display_name: 'website',
      recently_used_clone_path: '/home/dev/projects/website',
      clones: [
        {
          path: '/home/dev/projects/website',
          last_opened_at: '2026-01-01T00:00:00Z',
          last_branch: 'main',
          last_launch_option_ids: [],
          last_worktree_enabled: false,
          last_worktree_start_point: null,
        },
      ],
    },
  ];
}
