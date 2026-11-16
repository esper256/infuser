# Browser game with a Node server

Audience: game authors and their agents.

**Implemented and tested; SDK 1.0 remains a preview.** Use this recipe for plain JavaScript servers using HTTP/WebSocket, including
Three.js games using `ws`. Keep the game's networking messages and room logic.
The publisher uploads the client and server together; the author needs no Docker.

**Creating a game?** Use this recipe for backend design, then return to the
[creation guide](../create-game.md). Credentials are only needed for publication.
**Integrating an existing game?** Connect through the [publishing checklist](../agents.md),
apply this recipe and the dedicated-session guide, then return to
[Explicit play modes](../agents.md#explicit-play-modes) to finish the checklist.

Before publishing, read `/api/publisher/capabilities`. Require `server_packaging.enabled: true`
and a supported `node_major` from its recipes. If unavailable, report that specific
blocker. Do not relabel a server-backed game as client-only single-player.

## Prepare the existing game

Keep the public export and server source in separate folders. Do not put server
source, credentials or environment files in the public export. The server folder
contains its entry file, `package.json`, `package-lock.json`, and required local
modules. Do not upload `node_modules`.

Keep shared modules in both exports when the browser and server import them. Stage
the server folder with the relative paths its imports expect. For an unbundled
browser game, copy its browser dependencies into the public export too: URLs once
served by the Node development server (such as `/vendor/three.module.js`) must
exist in the static export. Do not copy the entire server dependency tree there.

The server listens on `process.env.PORT` on `0.0.0.0`. Its health endpoint returns
HTTP 200 once it can accept connections. It must start without platform credentials
for the isolated readiness probe and normal standalone development. Handle SIGTERM
and keep temporary files under `/tmp`; this recipe has no persistent world
volume. Personal cloud saves remain a separate game integration.

Example `infuser.game.json` additions:

```json
{
  "web_dir": "public",
  "play_modes": ["solo", "multiplayer"],
  "solo_runtime": "shared_server",
  "server_start": "on_demand",
  "multiplayer": {
    "mode": "dedicated",
    "transport": "websocket",
    "max_players": 8,
    "join_policy": "anytime"
  },
  "server": {
    "runtime": "node",
    "node_major": 24,
    "dir": "server",
    "entry": "server.js",
    "health_path": "/healthz"
  }
}
```

Merge this into the existing declaration, preserving title, SDK, saves, tags and
artwork. Use `client_only` when single-player genuinely needs no backend. A game can
add multiplayer in an update under the same app ID and publishing credential.

For verified identity, presence, reconnects and Join Friend, use
[dedicated sessions](../dedicated-sessions.md). Its browser helper warms the backend
and returns an ordinary WebSocket after admission. Add its manifest fields and
copy the two Node helper files into the server source. Keep SDK heartbeats running.
Disconnect and release the backend lease on returning to the title screen.
Use the [optional integration helper](../../sdk/optional.js) to retain standalone
play. A failed Infuser launch must not fall back to an unrelated public server.

Publish normally:

Use a [current publisher executable](../publisher-install.md). Older downloads may
only understand operator-staged images. The [HTTP route](../publishing-http.md)
also supports server source uploads without installing a CLI.

```sh
infuser publish .local/game.credentials.json /path/to/project
```

The CLI reports the build ID and packaging state, waits for readiness checks, then
publishes the matched client/server pair. A failure leaves the current release
active. The agent can inspect `/api/publisher/apps/APP/builds/BUILD/server-build`;
do not request another setup code for a packaging error. Fix source in a new build.
An interrupted build can retry its sealed inputs by POSTing that same URL.
For startup failures, crashes, or lost connections after publication, read
[server diagnostics](../server-diagnostics.md). The same publishing credential
can retrieve this game's live and recently stopped server logs.

The recipe installs registry dependencies from npm lockfiles v2/v3, offline, with
install scripts disabled. It does not support native compilation, Git/local/URL
dependencies, workspaces, project npm configuration or external backend services.
Managed servers can reach the provided Infuser API URL but have no Internet egress.
The [advanced staged-image route](../public-api.md) remains available for other needs.

Older games may keep the existing `runtime.acquire()` plus `/_infuser/server/`
transport. That route alone does not verify a browser player's identity or report
room membership. Use the dedicated-session helper for those features; do not
declare peer-hosted capabilities for a dedicated game. Both routes preserve the
game's own message protocol.

The [Node/WebSocket example](../../examples/dedicated-node/README.md) shows a complete
integration. Use your project's existing tests and the
[publication checks](../agents.md#publish-and-verify).
Report platform failures using [the agent issue instructions](../agents.md#reporting-platform-problems).
