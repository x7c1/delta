# Security & trust model

## Overview

delta-server runs on your machine and binds the loopback interface only. That
keeps it off the network, but loopback binding is *not* an authentication
boundary: any process or web page on the same host can still reach the port.
Delta therefore treats **reaching the loopback port as the trust boundary**
("unauthenticated-by-port") and layers explicit guards on top of it. This
document states what each guard covers, how a browser page comes to hold the
bearer token without any page carrying it, what the page's
Content-Security-Policy allows, how the live sockets and transcript reads are
bounded so one input cannot exhaust memory or block work, the one deliberate
trade-off in how
Delta pre-accepts Claude Code's workspace-trust dialog, how the files Delta
writes into its data directory are protected, how the desktop app checks an
update's signature and installs it (on Linux without the server ever holding
root, on macOS without root at all), what its logs
deliberately leave out, and how Delta handles the launch options that switch
an agent's own safety mechanisms off.

This is a living document: it describes the guards in place today, and later
hardening work will extend it.

## The loopback trust boundary

delta-server listens on loopback only, so a remote host cannot connect. Within
the local machine, four guards defend the surface:

- **Origin/Host guard** — rejects a request whose `Host` is not a loopback host
  (blocking DNS rebinding), and one that carries a *present* but non-loopback
  `Origin` (blocking a foreign web page from driving the API through the
  browser). A same-host non-browser client (e.g. `curl`) sends no `Origin` and a
  loopback `Host`, so it passes this guard — which is why the next one exists.
- **Per-run bearer token** — a secret minted once per server run and required on
  every REST call (`Authorization: Bearer <token>`) and live socket
  (`token=<token>` query parameter). No response served without credentials
  carries it; the page obtains it through a one-time code
  ([next section](#handing-the-token-to-the-page)). Anything without the valid
  token gets `401`. This closes the gap the Origin/Host guard leaves open for a
  local non-browser process. Three paths are exempt from *this* guard:
  `/health` (an unauthenticated liveness probe), `/hooks/*`, which carries the
  hook secret instead of a bearer token (next guard) rather than being left
  open, and `POST /api/auth/exchange`, which a page without a token calls to
  obtain one and which is authenticated by the one-time code instead.
- **Hook secret** — the `/hooks/*` control plane is called by Claude Code (not
  the browser), so it cannot carry a bearer token. Instead Delta renders a
  secret into the hook URLs (`?hs=<secret>`) and requires it on every hook
  request, giving that path its own authentication. Unlike the bearer token it
  is kept across restarts — in `delta-hook-state.json` in the data directory,
  owner-only (`0600`) — because a Claude Code session outlives Delta in tmux and
  keeps calling the URLs it was launched with. Deleting the file rotates it.
  The secret never appears on a command line: the `SessionStart` hook and the
  status line are shell commands that run `curl`, and a command line is
  visible to every local process (the status line runs one on every refresh),
  so each reads its URL from an owner-only `curl` configuration file next to
  the settings file (`curl -K <file>`), and every value Delta puts in those
  commands is shell-quoted. Only a secret of the form Delta mints (64 lowercase
  hexadecimal characters) is adopted, from the file or `DELTA_HOOK_SECRET`;
  anything else stops the start with an error naming its source. Like
  `DELTA_AUTH_TOKEN`, `DELTA_HOOK_SECRET` is removed from the environment once
  read, so no agent or command Delta starts can read it.
- **Scoped trust seeding** — the subject of the next section: Delta pre-accepts
  Claude Code's workspace-trust dialog only for worktrees it created itself,
  and only when you already trust what is in them or confirm it.

## Handing the token to the page

The page and its assets are served to any request (the browser cannot present
a token before it has one), so none of them carries the token. Instead the page
is opened at a URL with a **one-time code** in its fragment —
`http://127.0.0.1:<port>/#code=<code>` — and trades the code for the token:

- **The code.** Minted by the running server from the operating system's
  CSPRNG (well over 128 bits), compared in constant time, and valid for one
  exchange within 10 minutes of being minted, whichever ends first. A used,
  expired or unknown code answers `401` and returns no token. The fragment never
  reaches a server's request log or a `Referer`, and the page removes it from
  the address bar (`history.replaceState`) before it does anything else.
- **The exchange.** On boot the page posts the code to `POST /api/auth/exchange`
  and receives the token in the response body (`Cache-Control: no-store`). The
  endpoint stays behind the Origin/Host guard, so a foreign page cannot redeem a
  code either. In development it is under `/api/`, so the Vite dev server
  proxies it like the rest of the API.
- **Where the code comes from.** The desktop app runs the server in its own
  process, mints a code directly, and opens its window at the URL with it, on
  every launch. `delta-server` run directly prints the URL on one line of stderr
  when it starts. `delta-server open-url` prints a fresh one for the running
  server without restarting it (`--origin <url>` for another origin, such as
  the Vite dev server's); it authenticates to the server with the bearer token
  from the token file below, through `POST /api/auth/codes`. `make dev` prints
  the Vite URL with a code once both servers are up.
- **Where the page keeps the token.** In `localStorage`, which is scoped to the
  page's origin *including its port*, so two Delta servers on different ports
  never share it and a reload or a second tab of the same server keeps working.
  A cookie is deliberately not used: cookies are not scoped by port, so a cookie
  set on `127.0.0.1` would be sent to every other listener on `127.0.0.1`. The
  page presents the token exactly as before (`Authorization` header, `token=`
  on the socket upgrades). When it holds none, or the server answers `401`
  (it was restarted and minted a new token), the page shows a short screen
  asking for a new link instead of the app. A link that was already used falls
  back to the token the page already holds, so reopening an old link from a
  bookmark or the history still works while that token does; with none stored
  it shows the same screen. A refused socket upgrade only closes the socket in
  a browser, so each socket that drops on its own re-checks the token over REST
  and lets that `401` switch the page to the screen.
- **The token file.** For `open-url` and other local clients of the same user,
  `delta-server` writes its token to `<data dir>/auth-token` on start, created
  with mode **0600** inside the **0700** data directory, replacing any file an
  earlier run left. It removes the file on a clean stop (an erase, `SIGINT` or
  `SIGTERM`).
- **Not in child environments.** `DELTA_AUTH_TOKEN` fixes the token in advance
  for tests and scripts. The server removes it from its own environment as soon
  as it has read it, so the agents, tmux and every other command it starts do
  not inherit it.

## The page's Content-Security-Policy

Assistant output is attacker-influenceable (a prompt-injected agent can emit
Markdown that references a foreign URL), so the page carries a
Content-Security-Policy that keeps it from reaching any other origin: images,
fonts, fetches and sockets are same-origin only (`data:` images and fonts
aside), and the page cannot be framed. The per-directive rationale is in the
comment above the `<meta http-equiv>` policy in
`frontend/packages/apps/web/index.html`.

The policy is set in more than one place, and the browser enforces every one
it receives, so a resource must pass all of them:

- **The `<meta>` tag** in `index.html`, present in every serving mode. Its
  `script-src` keeps `'unsafe-inline'` and `'unsafe-eval'`, which Vite's dev
  client (its inline HMR client and React Refresh preamble) needs.
- **The response header from Vite** in development (`vite.config.ts`), the same
  policy, which also enforces `frame-ancestors` (a `<meta>` policy cannot).
- **The response header from delta-server** with the built page (`embed-web`).
  Its `script-src` has neither `'unsafe-eval'` nor `'unsafe-inline'`: it allows
  the page's own inline scripts — the theme bootstrap that sets `data-theme`
  before the bundle loads — by their `'sha256-…'` sources, so an injected
  inline script or event handler does not run. delta-server takes the hashes
  from the embedded `index.html` when it sets up the static surface, so editing
  the bootstrap cannot leave a stale hash behind, and a test checks them against
  the page's inline scripts. `style-src` keeps `'unsafe-inline'`, because xterm
  injects inline styles.

## Resource bounds

Everything behind the bearer token or the hook secret is still bounded, so one
token holder (or one runaway tab, or one odd file) cannot exhaust memory or
block the server's work.

**Live sockets.** Each of the three WebSocket endpoints
([live channels](api/live-channels.md)) bounds both the size of an inbound
message and how many sockets may be open at once:

| Endpoint | Largest inbound message | Open sockets |
| --- | --- | --- |
| `/ws` (events) | 4 KiB | 32, across all clients |
| `/comms` (comms log) | 4 KiB | 8 per session |
| `/pty` (terminal) | 1 MiB | 8 per session |

The events and comms streams are server → client, so they accept only control
frames inbound; the terminal carries keystrokes and pastes, so it allows a
generous paste. A frame whose header announces more than the bound is refused
before its payload is read, so the server never buffers it, and the socket is
closed. An upgrade over its endpoint's cap is refused with `429 Too Many
Requests` and a `warn` log naming the endpoint and the cap. A socket's slot is
released whenever it ends — a normal close, a receive error, or a crash in its
pump — so a failed socket never leaks one. The bounds are named constants in
`delta-server`'s `socket_limits` module, each with the reason for its size.

**Transcript reads.** A hook-reported transcript path must already lie under
the transcript root. The reader then opens it non-blocking and reads it only if
it is a regular file: a FIFO, a device or a directory is refused, so a FIFO
placed under the root cannot park the read (and the session behind it) waiting
for a writer. A transcript over 256 MiB (`MAX_TRANSCRIPT_BYTES` in
`delta-transcript`, far above a real long session) is neither read nor treated
as empty: it is refused with an error that the hook handler and the background
tail log. Every read takes the whole file, so without the cap one huge file
would make every hook cost in proportion to it. Lines are decoded one by one,
so a line that is not valid UTF-8 is skipped (logged at `debug` with its line
number) and the rest of the transcript still ingests.

## The Claude Code trust-seeding trade-off

Claude Code shows a blocking "Do you trust the files in this folder?" dialog the
first time it launches in a directory, and records the answer per absolute path
in the user's **global** `~/.claude.json`. A non-interactive launch (Delta's
spawn flow) cannot answer that dialog, so Delta pre-seeds the acceptance for the
directory it is about to launch in.

Writing that acceptance is consequential: it is global, so it also suppresses
the dialog in your own plain `claude` sessions in that directory — which means
any automation checked into that directory (a `.claude/settings.json` with
hooks) would then run without ever asking you.

Trust is also what lets a project's checked-in agent configuration — hooks, a
status line, `apiKeyHelper`, MCP servers, `CLAUDE.md` imports — take effect
without a prompt. And the content of a worktree made from a remote branch or a
pull request is decided by that branch's author, not by you.

Delta therefore pre-accepts the dialog only where you have, in effect, already
made the decision, and asks in its own UI otherwise:

- **A directory you point Delta at is never pre-accepted.** If you launch a
  session in an existing repository you selected yourself (or reuse a working
  tree outside the worktree base), Claude Code shows its normal one-time trust
  dialog; once you accept, it is remembered. Delta stays out of that decision,
  because pre-accepting it would also silently trust that repository's
  checked-in configuration in your plain `claude` sessions.
- **Rule A — a worktree inherits its repository's trust.** A worktree Delta
  creates under its worktree base is pre-accepted only when the repository you
  picked (its root) is itself trusted in `~/.claude.json`
  (`projects.<repo root>.hasTrustDialogAccepted` is `true`). A repository that
  has never been trusted gets Delta's confirmation instead. Confirming it also
  records trust for the repository root — only `hasTrustDialogAccepted`, the
  key Claude Code itself writes when you accept its dialog there — so the
  question is asked once per repository.
- **Rule B — a branch from outside is compared with what you trust.** For a
  worktree started from a remote branch (`remote_branch` or
  `use_remote_branch`, which covers every pull-request start), Delta compares
  the branch's tree with the tree of the repository root's `HEAD` — the content
  you trusted — on these paths:
  - everything under `.claude/` at the repository top, compared as a whole
    directory rather than as a list of known files, so a configuration file
    Claude Code starts reading later is still covered;
  - `.mcp.json` at the top;
  - every `CLAUDE.md` and `CLAUDE.local.md` anywhere in the tree.

  Names are compared case-insensitively, since a case-insensitive filesystem
  (the macOS default) treats differently cased names as the same file. When
  nothing differs — most branches touch none of these — the worktree is
  pre-accepted as under rule A, without asking. When anything differs, Delta
  asks first and shows the diff. A worktree off your own `HEAD` (`head`) needs
  only rule A.

**The confirmation.** Before a worktree session launches, the new-session flow
asks the server (`POST /api/launch-trust`, see
[sends.md](api/sends.md#post-apilaunch-trust)) whether the launch needs
confirmation. When it does, a dialog states why — the repository is not trusted
yet, and/or the branch changes agent configuration — and lists each changed
path with its unified diff. *Launch and trust* sends the session; *Cancel*
sends nothing, so no worktree and no session are created. The diff is computed
from git objects (`git diff` between the two commits, after the fetch the start
point runs anyway), never by reading files from a worktree, and external diff
drivers and textconv filters are off, so what is shown is git's own rendering
of the files. A very large diff is cut short and marked as such.

**The confirmation is bound to a commit.** The check reports the commit the
worktree will be on, and the launch request carries it back as
`worktree.confirmed_commit`. After building the worktree, the server applies
rules A and B again to the commit the worktree is actually checked out at. If
they still require a confirmation and that commit is not the confirmed one —
the branch moved after you looked, say — the server does not pre-accept and
the launch fails with a reason saying so; starting the session again (Retry
included) asks about what is there now.

**Resume.** Trust is per directory. Resuming a session in a worktree that is
already trusted does not ask again; a worktree that is not trusted is not
pre-accepted on resume either.

The first gate is a strict "is this path under Delta's worktree base?" check:
directories are canonicalized (so a `/tmp` vs `/private/tmp` symlink or a `..`
cannot disguise a path) and compared by path components (so a sibling like
`<base>-evil` is not mistaken for a child of `<base>`).

Only Claude sessions are affected: a Codex session never has Claude Code's
trust pre-accepted, so it is never asked about.

## Data-file hardening

Delta keeps every file it writes in its data directory (the layout is in
[the development guide](development/README.md#backend-backend)). Two of them
need more than an ordinary file:

- **The session settings file** — `<data dir>/settings/<port>.json`, the
  settings Claude Code is launched with. It embeds the hook secret in every hook
  URL, and its `statusLine` / `SessionStart` entries are commands Claude Code
  executes. So it is a secret-read surface *and* a command-injection surface.
  The `curl` configuration files those commands read
  (`settings/<port>-<endpoint>.curl`) hold the secret too, and are written the
  same way, every time the settings file is.
- **The tmux config** — `<data dir>/tmux.conf`, handed to tmux with `-f`. It
  holds no secret, but tmux executes every directive in it, so it is a
  directive-injection surface.

The server creates the data directory with mode **0700**, so by default no
other local user can reach either file. The writes do not rely on that, because
`DELTA_DATA_DIR` can point the data directory anywhere.

Delta creates the settings directory with mode **0700** and both files with mode
**0600**, and opens each file with `O_NOFOLLOW` so a symlink makes the `open(2)`
fail instead of redirecting the write. A settings directory that already exists
*as a symlink* is refused outright, since hardening only the file would leave the
directory as the swap target. An existing real directory is left as it is: it
may be one Delta does not own and is not Delta's to tighten. The permission bits
are re-applied on every write, because the creation mode does not touch a file
that already exists. The per-spawn working directories under
`sessions/` are created **0700** the same way, refusing a symlink.

That leaves one case open by construction, and only for a data directory pointed
at a place other users can write to: a `settings` directory another local user
pre-created — a real directory, not a symlink — is used as it stands. What Delta
writes there is still unreadable to them (0600), but the directory is theirs to
unlink from, so a file swapped in between Delta's write and Claude Code's read
would be the settings Claude Code launches with. Closing that would mean refusing
a settings directory Delta does not own, which Delta does not do today.

Both files are still rewritten on every run — the settings file must be, so that
the hook URLs carry the current run's secret.

### The rest of the data directory, and the logs

The data directory also holds the database (every prompt and reply Delta has
seen), its `-wal`/`-shm` files and its pre-migration snapshots, and on Linux the
desktop app's log directory sits in it too. None of them may be readable by
another local user, and the umask's default mode (typically `0644` for a file,
`0755` for a directory) would make them so:

- **The directory.** The desktop shell creates the data directory **0700** as
  the very first thing it does, before logging and Tauri start: both write into
  it on Linux (`logs/`, the webview's data) and would otherwise create it with
  the default mode. At every start the server makes an existing data directory
  it names itself, and its `logs/`, owner-only, which repairs a directory an
  earlier version left open. A directory named with `DELTA_DATA_DIR` keeps its
  mode, since it may be shared on purpose; `sessions/` and `updates/`, which
  Delta creates by name, are made owner-only wherever they are. A symlink in
  place of any of them is left alone, target included.
- **The database.** The database file is created **0600** before SQLite opens
  it, so the `-wal`/`-shm` files SQLite creates take the same mode, and after
  the open the database, both of those and every snapshot are set to **0600**,
  which also tightens files an earlier version left readable. A snapshot is set
  to 0600 as soon as it is written.
- **The logs.** The desktop app's log directory is **0700** and each daily file
  **0600**, made so before the logger writes to it.

What the logs leave out is in [log hygiene](#log-hygiene).

`~/.claude.json` is Claude Code's file, not Delta's, but Delta rewrites it to
seed trust ([above](#the-claude-code-trust-seeding-trade-off)). Each rewrite
writes a new temp file beside it — created exclusively, without following a
symlink, under an unpredictable name — with the file's current mode (**0600**
when Delta creates the file), then renames it into place, so a rewrite never
loosens the file's mode.

One more directory holds something that ends up mattering to root:
**`updates/`**, where the desktop app downloads a newer release and verifies
it against the sha256 digest GitHub states for it and against the release's
signature ([below](#signed-release-bundles)), keeping the signature beside
it. Whatever the user (or a
process running as the user) can write there could be swapped after that
check, so nothing that runs as root trusts it: the update helper below copies
the file out and checks the copy again. The macOS install, which runs as the
user, checks it again too before using it. Files there for a release the
running app is not older than are removed at startup. A download of the newer
release that an earlier run left there is not taken as verified either: once
the first release check names that release, its file is opened without
following a symlink, refused unless it is a regular file, and its sha256
compared with the release's digest and its contents checked against the
signature kept beside it again before it is offered for install; a file that
fails, or has no signature beside it, is removed, as is every file of another
release.

## Updates

The desktop app built by the release workflow can install a newer release on
Linux and on macOS (`POST /api/latest-release/install`, in
[the API reference](api/settings.md#post-apilatest-releaseinstall)).

Everything the update path fetches — the release feed, each signature, each
bundle, and the Linux helper's own requests — goes over HTTPS only: the
clients refuse an `http://` URL and a redirect to one, so nothing on the
network path can answer in GitHub's place. The feed the release check asks is
GitHub's, fixed in a `release` build: `DELTA_RELEASE_FEED_URL` may point a
`local` build at another `https://` feed (or turn the check off), but a
`release` build ignores any other URL with a warning, since the feed decides
which update the app offers to install.

### Signed release bundles

GitHub's sha256 digest of a Release asset is computed by GitHub from whatever
was uploaded. It proves the download arrived intact, not that the release
pipeline produced it: anything able to upload or replace an asset on the
Release would get a matching digest for its own file. So every bundle also
carries a signature, made with a key that no build step can read.

- **The key lives outside the build.** The release workflow signs each bundle
  with [minisign](https://jedisct1.github.io/minisign/) (Ed25519) in a job of
  its own, `sign`, the only job that may read the secret key: it is a secret
  of the GitHub Environment `release-signing`, which only protected branches
  may use. The job has no token permissions, checks nothing out and runs no
  project code; it only installs minisign from a pinned, checksummed release
  archive and signs the files the build jobs produced. The build jobs, which
  run the project's code and its dependencies' build scripts, never see the
  key. How the job is laid out is in [the release guide](release.md#signing).
- **What is signed.** Each bundle's signature, `<bundle>.minisig`, is
  attached to the Release next to it. Its trusted comment, which the
  signature covers, names the bundle's file name and the version:
  `file:<bundle> version:<version>`.
- **The public key is compiled in.** The desktop app and the Linux update
  helper both check against one public key, defined once in the
  `release-signature` crate and never read from a file or the environment:

  ```text
  RWSvjaqxlJm4wr9SiKBHDAlbhBASzeSf0dbg9loPJdgbTFQOEOX5J1Pl
  ```

  (minisign key id `C2B89994B1AA8DAF`).
- **The app checks before anything is offered for install.** It fetches
  `<asset>.minisig` from the same Release, under the same pinned URL prefix
  as the asset, before the asset itself, and checks the asset against it as
  it streams in. The signature must be made with the release key, must be a
  pre-hashed minisign signature, and its trusted comment must name exactly
  the asset's file name and the version being installed, so a valid
  signature of another file, or of an older release, is refused. A release
  without the signature, or one that does not verify, is never offered for
  install: the download ends `unavailable` with the reason, nothing is kept,
  and the user updates by hand from the release page. There is no fallback to
  the digest alone. On macOS this check comes before the disk image is copied
  or mounted; on Linux the root helper checks again by itself (below).
- **What it protects.** A bundle replaced on the Release after the release
  workflow built it, or uploaded by anything other than the release
  pipeline (a leaked or over-scoped token, a compromised account without
  access to the environment), is refused. So is a bundle served from
  anywhere but the Release, and an older signed bundle offered as a newer
  version.
- **What it does not protect.** The signature vouches for whatever the
  pipeline built. Code injected into the build itself — a compromised
  dependency, a malicious change merged to `main`, a tampered runner — is
  built and then signed like everything else. The GitHub artifact
  attestation the workflow also records (`gh attestation verify`) ties a
  bundle to the workflow run and commit that built it, which helps trace
  such a build but does not prevent it.

Checking a downloaded bundle by hand is described in the
[install guide](install/README.md#checking-a-download).

### Linux

Installing on Linux needs root, since the `.deb` is installed with
`apt-get`. Delta's server and UI
never hold root: the only program that runs as root for an update is a
dedicated helper, `/usr/lib/delta-desktop/delta-update-helper`, which the
`.deb` installs root-owned, and which the server starts through `pkexec`.

- **One polkit action, one program.** The `.deb` ships the polkit action
  `io.github.x7c1.delta.update`
  (`/usr/share/polkit-1/actions/io.github.x7c1.delta.update.policy`). Its
  `org.freedesktop.policykit.exec.path` names exactly the helper's path, so
  the authorization covers running that program and nothing else; and it
  asks for an administrator's password on every use (`auth_admin`, never
  `auth_admin_keep`), so no authorization outlives the install it was given
  for. The dialog's message names Delta's update.
- **The helper trusts only the version and the file.** It accepts one command
  line, `install --version v<version> --file <absolute path>`, and checks
  everything else itself, as root:
  1. it opens the file without following a symlink and refuses anything but a
     regular file, then copies it into a new directory (mode 0700) under its
     fixed working directory, `/var/cache/delta-update-helper`; every later
     step reads the copy, so the original cannot be swapped between the
     checks and the install. The working directory is created mode 0700, and
     the helper refuses to go on unless it and its parent are directories
     owned by root that no group or other user may write to;
  2. it asks GitHub for the release the tag names
     (`https://api.github.com/repos/x7c1/delta/releases/tags/<tag>`) and
     compares the copy's sha256 with the `sha256:<hex>` digest the release
     states for its `.deb` — the same digest the server checked the download
     against; a release that states none is refused;
  3. it downloads the release's signature of the `.deb` itself, only from
     the release's own download URL for it, and checks the copy against it
     with the release key compiled into the helper, as the app does
     ([above](#signed-release-bundles)); a release without one, or one that
     does not verify the copy, is refused. The helper takes no word from its
     caller that the file was checked: its command line has no place for
     one;
  4. it checks with `dpkg-deb` and `dpkg-query` that the package is
     `delta-desktop`, at exactly the requested version, and newer than the
     installed `delta-desktop`;
  5. it installs the copy with `apt-get install -y <path of the copy>`, so new
     dependencies resolve, and removes its directory.

  Each refusal and failure exits with a status of its own and prints one
  line saying why, which the app shows. The statuses `10`–`19` say the file
  itself failed a check (step 1, 2, 3, or 4's package name and version): the
  server then removes it and the app offers only to download the release
  again, never a command that would install that file. When the installed
  `delta-desktop` is already the requested version or newer (installed from
  a terminal, say), the helper installs nothing and exits with status `30`,
  which the app reads as installed and offers to restart into.
- **The helper takes nothing from its environment.** `pkexec` clears most
  of the environment, but the helper does not rely on that: it never reads
  `TMPDIR` (its working directory is fixed, above); its HTTPS client uses no
  proxy, whatever `HTTPS_PROXY`, `ALL_PROXY` and the like say, and trusts
  only the system's CA bundle at its fixed path
  (`/etc/ssl/certs/ca-certificates.crt`), never a file or directory
  `SSL_CERT_FILE` or `SSL_CERT_DIR` names; and it runs `dpkg-deb`,
  `dpkg-query` and `apt-get` by absolute path with an environment of its own
  (a fixed `PATH` and the C locale), so no variable left by its caller
  reaches them.
- **Why not `sudo` in a terminal.** Delta's tmux server hosts autonomous
  agents, and anything on that tmux socket can `send-keys` into any pane.
  `sudo` remembers an authentication per terminal for a while, so a Delta
  pane in which the user had typed their password would hand root to every
  agent on the socket.
- **Why not `pkexec apt install <file>` from the server.** The `.deb` sits in
  a directory the user can write to, so it could be swapped between the
  server's check and apt reading it — and its maintainer scripts run as root.
  An authorization to run `apt` would also cover running it with any
  arguments, not only this install.

When Delta cannot install the update itself — no `pkexec`, no polkit agent,
the helper missing, GitHub out of reach for the helper's checks, or `apt-get`
failing — the app shows how to install it from a terminal **outside Delta**
(not a Delta session's pane, which agents on the tmux socket can type into,
[above](#linux)): first the commands that check the file, `sha256sum <path>`
to compare with the digest on the release page and
`minisign -Vm <path> -P <release public key>` to check the signature Delta
kept beside it, then `sudo -k apt install <absolute path of the verified
.deb>`. `-k` makes `sudo` ask for the password and leaves no cached
authentication behind for anything else in that terminal to use. This way by
hand depends on nothing but the file, so it stays the way out even if the
in-app update turns out to be broken. It is not shown when the helper
rejected the file itself: a file that failed the check is not to be
installed as root by any route.

Restarting into the installed update starts the installed app,
`/usr/bin/delta-desktop`, after the running one has exited; nothing about it
runs as root.

### macOS

On macOS the update is a disk image (`Delta_<version>_aarch64.dmg`) holding
`Delta.app`, and installing it means replacing the running `Delta.app`. The
server does that itself, **as the user running Delta: nothing runs as root,
there is no helper, and Delta never asks for a password.** Where that user
may not write to the directory the bundle is in (a standard account and
`/Applications` owned by the admin group, say), Delta does not escalate: it
reports that it cannot install the update and shows the manual way below.

- **Which bundle.** The running bundle is found once, when the app starts and
  before any update replaced anything, from the server's executable
  (`<dir>/Delta.app/Contents/MacOS/<exe>` → `<dir>/Delta.app`); no path such
  as `/Applications` is assumed. A process that does not run from inside an
  app bundle has nothing to replace. A `Delta.app` opened straight from its
  disk image or the Downloads folder without being moved runs from a random,
  read-only copy macOS made of it (App Translocation, under
  `/private/var/folders/…/AppTranslocation/`); replacing that copy would
  change nothing, so Delta refuses and tells the user to move Delta to
  `/Applications` first.
- **No one else may change where the bundle is.** Delta installs only into
  a directory owned by the user or root that neither everyone nor a group
  may write to — except an administrators' group (`wheel`, `admin`), whose
  members can become root anyway, which is how macOS ships `/Applications`
  (`root:admin`, mode 0775) — unless its sticky bit keeps others from
  renaming what they do not own. Anywhere else another user could swap the
  installed `Delta.app` for an app of their own before it next starts, so
  Delta reports that it cannot install the update there, and why.
- **The file is checked again.** The user owns `updates/`, so the download is
  not trusted on the strength of its earlier check: it is opened without
  following a symlink, refused unless it is a regular file, and copied into a
  new directory only the user can reach while its sha256 is computed; the
  copy must match the digest the download was verified against. Everything
  after this reads the copy. A mismatch removes the download, and the release
  must be downloaded again.
- **The image is mounted privately and checked before anything is copied.**
  The copy is mounted read-only on a fresh directory, out of Finder's sight
  and without opening anything on it
  (`hdiutil attach -nobrowse -readonly -noautoopen -mountpoint <dir> <copy>`),
  and always detached again (`hdiutil detach`), whatever happens once it is
  attached. It must hold exactly one app at its root, a real `Delta.app`
  directory, whose `Contents/Info.plist` (read as a property list, not by
  running a tool on it) states `CFBundleIdentifier` `io.github.x7c1.delta`
  and `CFBundleShortVersionString` the requested version. Anything else
  rejects the image, and the download is removed.
- **The bundle is renamed, never overwritten.** The checked `Delta.app` is
  copied next to the running one under a temporary name with `ditto`, which
  keeps its symlinks, permissions and extended attributes and so its code
  signature. Then the running bundle is renamed to a backup name in the same
  directory and the copy to `Delta.app`: two renames on one volume, so the
  app is never half-written, and if the second fails the first is undone.
  The running app's files are never written to: overwriting a running,
  signed binary in place invalidates its signature and the kernel kills the
  process. The old process keeps running from the backup until it exits; the
  next launch removes the backup.

When Delta cannot install the update itself, or the install failed, the app
shows the absolute path of the verified `.dmg` with a copy button: open it,
quit Delta, drag Delta from it to Applications, then start Delta again. Like
Linux's command, it depends on nothing but the file. It is not shown when the
file or the image failed a check.

Restarting into the installed update `open`s the bundle found at startup
(`open <dir>/Delta.app`) after the running process has exited.

## Log hygiene

Server logs are a control-plane record, not a transcript: **a log line reports a
decision and its shape, never the content it acted on**. The `UserPromptSubmit`
hook response used to break that rule — it logged the `additionalContext` string
it returns to Claude Code, a verbatim excerpt of the conversation. It now logs
only whether context was injected and how long it was.

Two diagnostics in `delta-usecase` compare prompt texts: the prompt/send
mismatch line in `on_user_prompt_submit` (`info`, `expected` / `got`) and the
transcript-echo mismatch warning in `sync_transcript` (`warn`, `sent` /
`recorded`). Both fire only when Claude Code rewrote a text Delta itself
dispatched. At those levels they log each text's length and the first eight hex
digits of its SHA-256 — enough to tell whether and how much the texts differ —
and a `debug` line beside each carries the texts themselves, for a session that
needs the spellings. New logging around hook, prompt, or transcript handling
follows the same rule.

The configuration's secrets — the bearer token and the hook secret — are held in
a type whose `Debug` prints `***` and which has no `Display`, so logging the
configuration (`?config`) cannot print them.

`trace` is outside this rule by nature: at `trace` the log records what is typed
into a terminal and what it prints, and the payload of every frame on the live
channels (the WebSocket library logs frame contents at `trace`). Both binaries
log a `warn` line at startup when their filter enables `trace` for everything,
and such a log must not be shared as it is ([logging](development/logging.md#trace)).

The desktop app's log files are owner-only
([above](#the-rest-of-the-data-directory-and-the-logs)).

## Safety-bypassing launch options

A launch option is otherwise a pass-through — Delta does not read the names or
values, because the agent that receives them owns that vocabulary. A handful of
them, though, do not configure the agent so much as disable its guardrails:
Claude's `--dangerously-skip-permissions` (and `--permission-mode
bypassPermissions`), Codex's `sandbox = danger-full-access` and `approvalPolicy =
never`, and the same two settings written inside a Codex `config` value.

Delta's position is **marking, not prohibition**: those options stay registrable
and stay selectable for an individual session, because there are legitimate uses
and a tool that quietly removes the capability just gets worked around. What they
may never be is *silent*:

- **They cannot be enabled by default.** Setting `default_enabled` on such an
  option is refused (`400 launch_option_rejected`) on both the create and the
  update path, because a pre-checked bypass would disarm every new session
  without the user saying so even once. Turning the flag off is always allowed,
  so a row registered before this rule can be disarmed.
- **No shipped built-in is dangerous.** Startup reconciliation preserves each
  shipped row's `default_enabled`, which makes it the one writer that could
  reintroduce a pre-checked bypass past those refusals; a guard test in the
  composition root fails the build if a dangerous preset is ever added to a
  catalog.
- **They are marked, and never pre-checked, in the UI.** The registry badges such
  a row and disables its default control — except on a row that already carries
  the flag from before this rule, where the control stays live so the stored
  default can be cleared; the session-start picker badges it, refuses to
  pre-check it even if a stored row still says `default_enabled`, and reveals an
  inline warning naming the option once it is selected.

Recognising which `(name, value)` pairs qualify needs the provider's own
vocabulary, so the predicate lives in each agent gateway and is exposed to the
rest of Delta through one port. The verdict is **derived, never stored**: a row
registered before a spelling was recognised starts being flagged as soon as the
predicate learns it, with no migration. It is deliberately a closed list of
spellings that mean "stop asking" — an option that merely widens what is
auto-approved (`--permission-mode acceptEdits`) is not on it, because the
permission system is still running.
