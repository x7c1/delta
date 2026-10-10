# Logging

## Overview

Where Delta's log goes, how to change its level, and how to see which HTTP
requests the server receives.

- The `delta-server` binary logs to stdout only, filtered by `RUST_LOG`
  (default `info`). `make dev` and the e2e harnesses redirect it to files
  (`make dev`'s is `.tmp/delta-server.log`; see [local-run.md](local-run.md#launch)).
- The desktop app logs to stdout **and** to a daily file in the platform's app
  log directory. Its level comes from `RUST_LOG` when set, otherwise from a
  `log-filter` file in its data directory, otherwise `info`.
- Every HTTP request is logged at `debug` under the `delta_server::http`
  target. `info,delta_server::http=debug` turns that log on and nothing else.

## The desktop app's log file

The file is `delta-desktop.<YYYY-MM-DD>.log` in the directory Tauri's path
resolver names as the app log directory:

| OS | Directory |
| --- | --- |
| macOS | `~/Library/Logs/<identifier>` |
| Linux | `${XDG_DATA_HOME:-~/.local/share}/<identifier>/logs` |

`<identifier>` is `io.github.x7c1.delta` for the installed app and
`io.github.x7c1.delta.dev` for the dev build (`make desktop-dev`). A new file is
started each day and only the last seven are kept. Every line also goes to
stdout, so the Linux desktop session's journal and the terminal running
`make desktop-dev` still show it.

The file is written by a background worker, so a request never waits on the
disk; what the worker still holds is written out when the app quits. When the
directory cannot be created or the file cannot be opened, the app logs a
warning to stdout and carries on with stdout only. Erasing everything from
Settings → Storage removes the directory with the app's other files.

## Changing the level

`RUST_LOG` wins whenever it is set, for both the desktop app and the
`delta-server` binary.

An app started from the launcher, the Dock or Finder has no `RUST_LOG`, so the
desktop app also reads [`EnvFilter`] directives from `log-filter` in its data
directory (`DELTA_DATA_DIR` when set, else
`<platform data dir>/<identifier>` — `~/Library/Application Support/<identifier>`
on macOS, `${XDG_DATA_HOME:-~/.local/share}/<identifier>` on Linux):

- Directives may be on one line, comma-separated, or on several lines, which
  are joined with `,`. A line starting with `#` is ignored.
- A missing or empty file means `info`.
- A file that does not parse logs a warning naming the file, and the app logs
  at `info`.
- The file is read once, at launch: a change takes effect on the next launch.

The `delta-server` binary does not read `log-filter`. It is always started from
a shell or a script (`make dev`, the e2e harnesses), where `RUST_LOG` can be
set directly.

At launch the desktop app logs the filter in use and where it came
from (`filter_source`: `RUST_LOG`, the file's path, or `default`) and the log
directory, in a `delta-desktop logging` line.

[`EnvFilter`]: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html

## The request log

Each request the server answers — the API, the hooks, the WebSocket upgrades
and the static web frontend — is logged once its response is ready, including a
request the bearer-token guard (`401`) or the origin guard (`403`) refuses:

```text
2026-10-10T10:20:31.512843Z DEBUG delta_server::http: request method=GET route="/api/sessions/{id}/threads" path="/api/sessions/0199d0a4-5b7e-7c31-9a3f-2f6e1c0d8b44/threads" status=200 elapsed_ms=3.4
```

- `route` is the route template the request matched, so lines can be grouped by
  route; `-` when no declared route matched (a static asset, or a `404`).
- `elapsed_ms` is the time to the response. For a WebSocket upgrade it is the
  upgrade alone; the socket's life is not logged.
- Only the path is logged, never the query string or a header: the WebSocket
  upgrades carry the bearer token in their query, the hooks their secret, and
  other routes carry filesystem paths.

To turn it on in the desktop app, write the directive into `log-filter` and
relaunch. For the installed app on Linux:

```bash
echo 'info,delta_server::http=debug' > ~/.local/share/io.github.x7c1.delta/log-filter
```

On macOS the file is
`~/Library/Application Support/io.github.x7c1.delta/log-filter`. Remove the
file (and relaunch) to turn it off again. For the `delta-server` binary, set
`RUST_LOG` instead; `make dev` passes it on to the server:

```bash
RUST_LOG=info,delta_server::http=debug make dev
```
