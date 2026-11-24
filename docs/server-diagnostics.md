# Debug a dedicated server

Audience: game authors and their agents.

Use the game's existing publishing credential. No SSH or new setup code is needed.
Check `server_diagnostics.version === 1` in `/api/publisher/capabilities` before
using the runtime endpoint. These reports are private to that game's publisher;
player, metadata-only and other-game credentials cannot read them.

## Packaging or publication failed

Read `GET /api/publisher/apps/APP/builds/BUILD/server-build` using the build ID
printed by the CLI. It returns the recipe, attempt count, state, error and the
last 32 KiB of logs. Successful jobs also identify the image, base image and lockfile
hash. Save the report before retrying: a retry starts a new log for that build.

- `queued`: waiting for the worker or available host CPU/RAM. Do not mint another app.
- `failed`: read the log. Check imports, package/lockfile agreement, `PORT`,
  `0.0.0.0`, and the declared health endpoint. Installation scripts do not run.
- `packaging_interrupted`: retry the same sealed inputs by POSTing `server-build`.
  Source changes require a new build ID.
- Ready but publication `queued`: the old server is still in use. It is not an
  upload failure; the matched update activates after that server sleeps.

## Game starts badly, disconnects, or crashes

Read `GET /api/publisher/apps/APP/server-diagnostics`. The response contains:

| Field | Meaning |
| --- | --- |
| `active_build`, `queued_build` | Published and waiting client/server versions |
| `runtime` | Lifecycle phase, attempt, image, lease count, CPU/RAM limits and controller error |
| `live` | Current container report, or null when no container is available |
| `live_error` | Capture failed; null does not imply the game itself is healthy |
| `recent` | Up to three completed runtime attempts, newest first |

Each container report includes its build ID when packaged by Infuser, attempt,
capture/start/finish times, health checks, exit code, OOM flag, and stdout/stderr.
The last 200 Docker log entries are requested, capped to the last 16 KiB per stream;
`logs_truncated` flags that byte cap. This is a diagnostic tail, not a complete log.
`logs_error` means log retrieval failed or timed out. Docker environment variables,
credentials and complete inspect output are never included by the collector.

Reports survive idle cleanup, crashes and controller restarts. The three-attempt
history is replaced as new runs finish; it is not a permanent audit archive. A
missing report can mean the container was never created or was removed externally.
Logs cannot explain game logic that the game never recorded.

Write useful server events to stdout/stderr: startup, room admission failures,
connect/disconnect, caught errors and shutdown. Include a stack for unexpected
errors. Avoid logging every simulation tick, packet or snapshot. Never log tokens,
cookies, room passwords, save contents or the process environment. Use the browser timeline below for client errors and matching request IDs.

Keep reports private. When [filing a platform issue](agents.md#reporting-platform-problems),
include the build ID, runtime attempt, approximate time, failing stage and a short
sanitized excerpt. Do not paste raw logs, credentials or publication captures into
a public issue.

A healthy server with a failed browser connection can still indicate a client,
authentication or proxy problem. Include the failing URL path and HTTP status;
do not include its query, launch fragment or authorization headers.

## Browser and game timeline

For web games, check `diagnostic_events.version === 1` in publisher capabilities. Use the game's existing publisher credential to GET `/api/publisher/apps/APP/diagnostic-events`. Events cover up to 48 hours and arrive oldest first. Narrow a request with `since_ms` and `until_ms` (Unix milliseconds). The default page size is 200, with a maximum `limit` of 500. Pass the returned `next` as the next request's `cursor` until it is null. Keep the time window fixed when paging.

Look at the app build, launch reference, error location, multiplayer events, API status and runtime attempt. Use the matching attempt in server diagnostics for container output. Browser errors include a fingerprint, exception class and available line and column. Household activity stays private. A missing event is inconclusive when a browser was offline or closed. Use this evidence before asking a player to reproduce a problem.
