# Put an existing game on Infuser

Start with the game's engine. Do not ask the child to configure a server, Docker,
DNS, or ports. Keep their existing game and build workflow intact.

1. Read the private `*.credentials.json` game pass supplied separately. It contains
   `api_url`, `app_id`, and a publishing token for **one game**. Keep it outside the
   game source/export, or in a Git-ignored `.local/` directory. Never print it.
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

The child can create a game pass and copy this task from **Add a game** in the
clubhouse. Only adult admins can invite or disable members. A disabled member's
publisher credentials and player sessions stop working.

## What Infuser handles

- HTTPS, game hosting at `/` on an isolated hostname, MIME types, range requests,
  health-aware server startup, and idle shutdown.
- A small browser SDK, automatically included in the uploaded entry page by
  `infuser publish`, maintains the session while the game is open.
- Cloud-save namespaces bound to the signed-in player and game; guest play gets a
  new empty namespace **and a new browser origin**. Never copy local progress
  into guest saves or fall back to the owner's identity.
- Immutable builds. Existing browser sessions keep their selected build. Server
  updates return `409` while the shared server is active; retry after it sleeps.
- One shared server per game. A second player joins it; never create a parallel
  lobby/world merely because a connection fails.

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
connection is an optional optimization. Peer-hosted and Infuser datagram/relay
services are **not installed yet**. There is no LAN-only/direct-only fallback.

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
