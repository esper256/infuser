# Game SDK: path to 1.0

**Status: pre-1.0, experimental.** The current browser SDK and game-facing HTTP
API are a dogfood contract, not a stable SDK release. Manifest `schema_version: 1`
describes a document format; it does not mean SDK 1.0. Firefly is disposable and
may be reset or republished while we settle this design.

The launcher can evolve quickly. Contracts embedded in uploaded games need a
short design review before release: concrete request/response examples, failure
behavior, and what happens to an older game. This is a focused review of durable
interfaces, not an approval process for ordinary UI changes.

## Review before calling the SDK 1.0

Approved product decisions:

- The game-making agent owns building and deciding when to publish. Infuser
  provides an agent-invoked CLI/API accepting runnable artifacts, validation,
  immutable releases, and safe activation. Build detection is an optional shortcut;
  game publishing never requires GitHub or an Infuser-owned development watcher.
- Save conflicts preserve both candidates and let the game offer a choice; no
  silent overwrite or generic merge. Games declare their save files so Infuser
  can load and commit coherent cloud snapshots. See the concrete
  [cloud-save contract](cloud-save-design.md). A reusable chooser is desirable;
  descriptions and any merge remain game-owned.
- Guest progress lasts for the current launch, including refreshes. A new guest
  launch starts empty, has a distinct identity, and may join the shared multiplayer
  session. Guest progress never merges into the signed-in owner's saves.
- Peer host departure ends the session in the first version. Explicit rejoining
  is supported; seamless host migration is a later declared capability.
- Join friend is an intent handled by the game. Failure returns visibly to its
  title screen and never creates a second shared world.
- Stable SDK majors coexist. Additions are capability-gated; an unchanged older
  game must keep working when the daemon updates. Overall SDK 1.0 is not frozen yet.
- Multiplayer updates upload immediately and activate after the shared session
  ends and its server shuts down. Queued publication is implemented; a force-restart
  action is reserved for adult administration.
- Online, game open, and joinable multiplayer are separate states. The game must
  explicitly report joinability. A title screen does not create a multiplayer room.

| Contract | Decisions to settle |
| --- | --- |
| Identity and guest play | Stable opaque player IDs; guest lifetime and identity; app scoping; which identity a game server may trust |
| Cloud saves | Empty saves, compare-and-swap conflicts, error codes, limits, game-owned schema migrations, recovery and rollback behavior |
| Sessions | Title-screen startup, explicit backend acquisition/release, readiness, reconnect, timeout and session expiry |
| Multiplayer and presence | Online friends versus players in this game versus a joinable multiplayer session; capacity and joinability; one shared session; tickets and relay availability |
| Launch intent | Ordinary Play opens the game's title screen; optional Join friend intent only for games declaring support; unavailable/full sessions fall back visibly |
| Compatibility | Manifest declaration of required SDK/API major and optional capabilities; versioned SDK and HTTP routes; admission rules for unsupported requirements |

Presence must not equate “has this game open” with “playing multiplayer.” A title
screen can show friends and joinability without creating a server. The game
requests its backend when the player chooses a mode; leaving multiplayer releases
that need without ending an unrelated single-player run. Join friend is a request
to join the existing shared session, never permission to create another world.
Exact method names and payloads remain proposals until reviewed.

Today, Play has no mode picker; the game owns that choice. The pre-1.0 launcher
still warms a declared multiplayer backend before opening the game. On-demand
acquisition from inside a title screen, in-game presence, and Join friend intents
are not implemented yet. Existing explicit-mode launch requests remain accepted.

## Compatibility promise to implement before 1.0

- Version the game SDK and wire protocol independently of daemon releases.
  Uploaded builds declare their required major; never silently bind them to a
  newer incompatible major or a moving unversioned implementation.
- Within a supported major, preserve existing behavior, fields, meanings, and
  save ownership. New optional fields/features are additive and capability-gated.
  A missing optional feature must not stop the game from launching.
- A breaking change gets a new major alongside the existing adapter. Upgrading
  Infuser must not require every game-making agent to republish its game.
- Review persistent data migrations with both upgrade and rollback in mind.
  Controller binary rollback alone cannot undo a save or world migration.
- Keep one small frozen old-client journey: launch, identity, save/reload,
  guest isolation, and session renewal against the new daemon. Add cases when
  a durable new contract ships; keep the checks fast.

The current `/_infuser/sdk.js` and unversioned game routes do **not** provide this
promise yet. Do not advertise them as stable or start a second engine adapter
while the first is still changing. Settle and exercise the contract with
Three.js/Vite before expanding compatibility. During review, consider whether
the wire format is usable from other engines without implementing their adapters.

## Later features: achievements as an example

Achievements are not implemented. Before releasing them, review stable app-scoped
achievement IDs, idempotent unlocks, who can authorize an unlock, guest behavior,
metadata edits versus retired IDs, and how progress survives game updates. These
are design questions, not an API already available to agents. An older game that
never calls an achievements API must continue working unchanged.
