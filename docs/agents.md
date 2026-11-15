# Put an existing game on Infuser

Start with the game's engine. Do not ask the child to configure a server, Docker,
DNS, or ports. Keep their existing game and build workflow intact.

The game-making agent owns the build and publishing loop. “Update it on Infuser”
means build using the project's existing tools, then invoke `infuser publish`.
“Publish every update” is an instruction to that agent, not an Infuser watcher.
Supply prebuilt output with `--web-dir dist` (or the project's actual export path);
automatic build detection is only a convenience. GitHub is not required for games.

Current development is focused on stabilizing Three.js/Vite. Existing Godot/Bevy
notes are preliminary; do not expand those adapters while the first engine's
publishing, saves, sessions, and SDK contract are still changing.

1. Reuse existing credentials for this app if present. Otherwise the copied prompt
   contains a one-time setup code, server origin, and app ID. Supply the code on
   stdin to `infuser connect --server HTTPS_ORIGIN --app APP`. The CLI exchanges it
   once and stores `.local/APP.credentials.json`. Do not echo secrets into logs or
   shell history. Ignore `.local/` and `*.credentials.json` in Git and exports.
   The code expires in one hour; the child can use **Refresh code** for the same
   app. An operator-supplied credentials file also works. Never create another
   app just to refresh credentials.
2. Inspect the project, then choose the closest recipe:
   - **[Three.js / Vite / browser JavaScript](engines/threejs-vite.md)** — first supported journey.
   - **[Godot web export](engines/godot-web.md)** — export and adapter path.
   - **[Godot multiplayer](engines/godot-multiplayer.md)** — explicit transport choices; recipe in progress.
   - **[Bevy](engines/bevy.md)** — browser export first; recipe in progress.
3. Query `GET /api/publisher/capabilities` with the pass's bearer token before
   choosing a multiplayer integration. Engine choice does **not** tell you the
   game's topology. Solo and multiplayer may both be supported.
4. Use the Infuser CLI matching this server's current release. Linux AMD64/ARM64
   executables are attached to this repository's releases; verify their SHA-256
   against `release.json`. Other development platforms can build the CLI with Rust.
5. Integrate saves, publish, then exercise a real player journey. An upload alone
   is not completion. Use `--preview` for admin-only dogfood/test games.

The child copies this task from **Add a game** in the Library; no files to attach.
Only adult admins can invite or disable members. A disabled member's
publisher credentials and player sessions stop working.

**SDK status: pre-1.0.** Read the [compatibility review](sdk-contract.md). The current
integration is for dogfooding; do not mistake manifest schema 1 for SDK 1.0 or
invent APIs for achievements or launcher Join friend intents. Play opens the
game's own title screen; keep its Single player/Multiplayer selection intact.

Infer a few initial tags from the actual game. Prefer the supplied catalog or
`GET /api/publisher/tags`; put up to eight tag IDs in `infuser.game.json`, for
example `"tags":["3d","arcade","chill"]`. Later publishes preserve author edits.

## What Infuser handles

- HTTPS, game hosting at `/` on an isolated hostname, MIME types, range requests,
  health-aware server startup, and idle shutdown.
- A small browser SDK, automatically included in the uploaded entry page by
  `infuser publish`, maintains the session while the game is open.
- Cloud-save namespaces bound to the signed-in player and game; guest play gets a
  new empty namespace **and a new browser origin**. Never copy local progress
  into guest saves or fall back to the owner's identity.
- [Declared JSON checkpoints](cloud-save-design.md), when capabilities advertise
  `cloud_checkpoints.version = 1`. Submit complete groups, handle preserved
  conflicts explicitly, and reuse the same commit ID when retrying a write.
- Immutable builds. Existing browser sessions keep their selected build. Server
  updates are queued while the shared server is active and activate automatically
  after it sleeps. Check `publication_state` on the build to distinguish an upload
  from an activated update; the CLI reports this distinction.
- One shared server per game. A second player joins it; never create a parallel
  lobby/world merely because a connection fails.
- [On-demand managed server sessions](runtime-sessions.md): opt in with
  `"server_start":"on_demand"`, then acquire when a mode needs the backend and
  release when it does not. Title screens and client-only single player use no
  server reservation. Peer-hosted games use `peers.join()` instead.
- [In-game presence and peer joinability](presence.md): query `presence.get()`
  on the title screen, distinguish online/game open/multiplayer, and join the
  observed session ID. Only the host advertises when new players may join.

## Explicit play modes

For an existing solo game, `--solo` is the only gameplay declaration needed.
For dual-mode or multiplayer games, supply `infuser.game.json`, for example:

```json
{
  "title": "Moon Racers",
  "play_modes": ["solo", "multiplayer"],
  "solo_runtime": "client_only",
  "multiplayer": {
    "mode": "dedicated",
    "transport": "websocket",
    "max_players": 8,
    "join_policy": "anytime"
  }
}
```

A server recipe adds its managed server settings. Do not invent support for an
unavailable runtime. `peer_hosted` always requires relay fallback, even if direct
connection is an optional optimization. Experimental [WebRTC peer hosting](peer-hosting.md)
is available on installations configured with TURN; check capabilities first.
The separate datagram/WebSocket relay mode is not implemented. There is no
LAN-only/direct-only fallback.

## Updates and cleanup

Repeat the same publish command using the same game pass. The CLI creates a new
immutable build and promotes it. A failed upload leaves the working game alone.
An abandoned build can be inspected/deleted through the publishing API.

```sh
infuser publish /private/game.credentials.json /path/to/project --solo
infuser publish /private/game.credentials.json /path/to/project --solo --preview
infuser unpublish /private/game.credentials.json
```

Unpublish removes the game from the library and expires its sessions, preserving
builds and personal saves. Unused builds can then be deleted to reclaim disk.

The low-level [public API](public-api.md) is the escape hatch for custom pipelines,
not the starting point. Never give a game the operator control token or its
publishing credential. Keep individual game secrets out of container images.
