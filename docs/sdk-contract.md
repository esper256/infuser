# Game SDK: path to 1.0

Audience: Infuser developers.

**Status: pre-1.0, experimental.** The current browser SDK and game-facing HTTP
API are a dogfood contract, not a stable SDK release. Manifest `schema_version: 1`
describes a document format; it does not mean SDK 1.0. Preserve published games
and saves while these contracts settle. Use separate disposable fixtures for tests.

The launcher can evolve quickly. Contracts embedded in uploaded games need a
short design review before release: concrete request/response examples, failure
behavior, and what happens to an older game. This is a focused review of durable
interfaces, not an approval process for ordinary UI changes.

The [accepted SDK 1.0 decisions](sdk-1.0-decisions.md) guide the
[stabilization plan](proposals/sdk-1.0-stabilization.md), which separates implemented
preview contracts from the evidence needed to freeze them. The
[dedicated-server proposal](proposals/sdk-1.0-dedicated-servers.md) records the
earlier design; [dedicated sessions](dedicated-sessions.md) describes the shipped
candidate. Publishing inputs can be retained as
[private importer fixtures](publication-capture.md).

The [temporary-visit design](proposals/temporary-visits.md) is implemented with
scoped deployments, saves, presence, and expiring access. Preserve those boundaries
in compatibility acceptance before freezing the affected contracts; existing
home-group saves and games stay compatible.

## Review before calling the SDK 1.0

Approved product decisions:

- Player IDs identify permanent group profiles, not Google accounts or email
  addresses. Existing IDs and save ownership survive the identity migration.
  Games must treat them as opaque; provider linking and profile selection belong
  to Infuser. Separate profiles have separate IDs, games and progress.
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
- Existing networking libraries can have explicit adapters. The [PeerJS preview](peerjs.md)
  preserves PeerJS DataConnections while sharing Infuser admission and relay rules.
  `peers.peerjs.v1` gates the new adapter; existing manifests and peer helpers retain
  their behavior. Games can select an optional platform integration at launch and
  keep their standalone workflow. A failed hosted session must not silently fall
  back to public matchmaking or a different save namespace.
- Multiplayer updates upload immediately and activate after the active sessions
  end and their managed server shuts down, or the peer room ends. Queued publication is implemented; a force-restart
  action is reserved for admin administration.
- Online, game open, and joinable multiplayer are separate states. The game must
  explicitly report joinability. A title screen does not create a multiplayer room.

| Contract | Decisions to settle |
| --- | --- |
| Identity and guest play | Stable opaque player IDs; guest lifetime and identity; app scoping; which identity a game server may trust |
| Cloud saves | Empty saves, compare-and-swap conflicts, error codes, limits, game-owned schema migrations, recovery and rollback behavior |
| Sessions | Title-screen startup, explicit backend acquisition/release, readiness, reconnect, timeout and session expiry |
| Multiplayer and presence | Online versus game-open versus joinable; independent peer rooms or shared/multiple dedicated runs; atomic capacity, tickets and relay availability |
| Launch intent | Ordinary Play opens the game's title screen; optional Join friend intent only for games declaring support; unavailable/full sessions show an error and return to the title screen |
| Compatibility | Manifest declaration of required SDK/API major and optional capabilities; versioned SDK and HTTP routes; admission rules for unsupported requirements |

Presence must not equate “has this game open” with “playing multiplayer.” A title
screen can show friends and joinability without creating a server. The game
requests its backend when the player chooses a mode; leaving multiplayer releases
that need without ending an unrelated single-player run. Join friend is a request
to join the exact observed session, never permission to create another world.
The linked contract pages describe implemented preview methods and payloads.

Today, Play has no mode picker; the game owns that choice. Managed games can opt
into [on-demand runtime sessions](runtime-sessions.md): the title screen opens
without a backend, and the game acquires/releases one when needed. Existing builds
retain launch-time warming and explicit-mode launch requests remain accepted.
Peer-hosted games already enter multiplayer explicitly through `peers.create()` (host) or `peers.join()` (join). Games using explicit peer rooms require `peers.rooms.v1`.
[In-game presence and explicit peer joinability](presence.md) are implemented
experimentally. A targeted peer join binds to the observed session ID; it never
creates a replacement world. [Join friend launch intents](launch-intents.md) are
opt-in and consumed once by the game, with guarded friend/session admission.
[Dedicated sessions](dedicated-sessions.md) adds verified Node/WebSocket admission,
room reports, exact Join Friend and reconnects through the `sessions.v1` and
`connections.websocket.v1` capabilities. One managed process can host shared or
explicitly multiple runs. The [managed Node recipe](engines/node-server.md)
packages both client and server; the game's gameplay messages remain unchanged.

## Compatibility foundation and promise for 1.0

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

The [versioning foundation](sdk-versioning.md) is implemented: explicit SDK/API
majors, capability admission, separate v1 imports/routes, and a frozen old-client
journey. Legacy imports/routes remain available. Major 1 is still a preview, not
a declaration that the full SDK contract is frozen. Review the durable semantics
above before calling it stable. The [Godot web study](engines/godot-web.md) is
explicitly experimental and exercises the same wire contract before that freeze;
native authentication and artifact delivery are implemented for single-player
Godot on Windows/Linux. Binary checkpoints are implemented; native multiplayer remains open.

## Later features: achievements as an example

Achievements are not implemented. Before releasing them, review stable app-scoped
achievement IDs, idempotent unlocks, who can authorize an unlock, guest behavior,
metadata edits versus retired IDs, and how progress survives game updates. These
are design questions, not an API already available to agents. An older game that
never calls an achievements API must continue working unchanged.
