# Product boundaries

Infuser uses familiar Steam-style concepts, with its own small HTTP contracts.
It is not Steamworks-compatible and does not require Steam.

The current game SDK is pre-1.0. See the [contract review](sdk-contract.md) before
adding durable game-facing features and the [public API](public-api.md) for
implemented routes. Planned concepts below do not imply runtime availability.

| Concept | Meaning in Infuser |
| --- | --- |
| App | Stable identity of a game; independent of engine and release |
| Build | Immutable, validated set of artifacts |
| Depot/artifact | Web client, native client per platform, or server image |
| Branch | Mutable pointer such as `preview` or `default` to a build |
| Lobby | The shared gathering place for an app's multiplayer players |
| Session | The app's single active multiplayer match or persistent world |
| Presence | Online, playing an app, joinable lobby/session |
| Cloud save | Player-owned data scoped to app and save slot |

## Deliverables

1. **Rust daemon/API:** resource budgets, game lifecycle, ports, readiness and
   recovery. The first slice implements this, with one server instance per app.
2. **Website:** Google login, invited members, library, friends, presence and Play.
   UX can evolve without changing the server runner.
3. **Agent publishing API and CLI:** create apps, upload artifacts, validate builds,
   promote branches, inspect logs and roll back. Publisher credentials grant access
   only to owned apps. Agents never receive the Docker socket or host shell.
4. **Agent instructions and examples:** a short entry point, manifests, build
   recipes and actual working examples. Every documented API says whether it exists.
5. **Game adapters:** JavaScript first, then Godot and Rust/Bevy. Common concepts
   include player identity, lobbies, sessions, admission tickets, saves and presence.

The website and publishing client use the same public platform API but different
credentials and permissions. Privileged container execution stays behind that
API, with a loopback-only controller boundary. Keep this one deployable service
initially if convenient; these are responsibilities, not a requirement for a
microservice per feature.

## Build and runtime contracts

Player modes and multiplayer architecture are separate. An app declares
`play_modes: ["solo"]`, `["multiplayer"]`, or both. If solo is supported,
`solo_runtime` explicitly states `client_only` or `shared_server`; solo does not
automatically mean offline or permission to start a second server.

If multiplayer is supported, the manifest declares `mode`, `transport`,
`max_players`, and `join_policy` (`anytime` or `between_rounds`). The architectures
are `peer_hosted`, `relayed`, `dedicated`, and `persistent_world`. Engine choice
never substitutes for these declarations.

Peer-hosted games must support Infuser-reflected traffic. They may attempt a
direct connection first as an optimization, but relay fallback is mandatory;
IP hiding is not a requirement. Integration checks must succeed with direct
connections disabled. A relay-capable WebRTC transport uses Infuser signaling
and TURN; the WebSocket alternative must implement the Infuser relay protocol.
LAN-only discovery, required port forwarding and raw peer TCP/UDP are unsupported.
Dedicated public servers can still accept outbound client TCP/UDP connections.

`infuser validate-game examples/game-manifest.json` validates the explicit
contract. A valid declaration does not imply that its runtime is installed:
`GET /v1/capabilities` reports availability, and currently marks peer hosting and
relaying unavailable until those services exist. Publishing must reject unavailable
modes rather than silently selecting another topology.

Pure web games are static files served at the root of their own hostname, without
an always-running game container. An app can also have a server process. Native
clients can use public TCP/UDP mappings; browser backends use HTTPS/WSS via Caddy.
WebRTC signaling and TURN are separate future services, not something supplied
automatically by Docker port mappings.

An immutable server build is an image digest with an architecture, resource
request, internal port/protocols, readiness health check and shutdown contract.
Each installed app gets a stable host port from the configured range; sleeping
apps keep their slot but consume no game CPU/RAM. This makes route configuration
stable. One server per app is an intentional product rule, not a temporary
scaling limitation.

## One shared multiplayer session per app

Within an Infuser installation, an app has at most one active multiplayer session.
The first player starts it; everyone else joins it. The launcher can show **Play**
when empty and **Join friends** when occupied, with player names and capacity.
Different games can run concurrently within the resource budget. Solo games still
run independently on each player's device.

Dedicated servers already share one instance across player leases. Future lobby
and peer-hosting APIs must enforce the same rule: one authoritative host/session
record per app, claimed atomically even when two players launch simultaneously.
Multiple rooms hidden inside one server would also defeat the social rule; game
adapters should gather players into the same multiplayer space.

When a game is full or cannot accept players mid-round, offer spectating if the
game supports it, otherwise wait for a seat or the next round. Do not create
another session as a fallback. The game reports capacity and joinability; these
are distinct from the controller's lease limit and process readiness.

For peer-hosted games, host departure requires coordinated host migration with
the old host fenced out, or ending the session before a new host claims it.
Never elect a second host just because a join attempt times out. Peer signaling,
host migration, spectating and waiting-room behavior are planned features.

Publishing stages and verifies a new build before moving the active pointer. The active
multiplayer session stays on its build, and new arrivals must launch a compatible
client for that same build. A promoted build takes effect after the session ends
or during a coordinated restart; it must not spawn a parallel server. Hosted
preview branches share this same app-level restriction; test locally while the
shared session is occupied. Persistent worlds
need explicit checkpoint/migration/restore support. Binary rollback is not a
database rollback. Uploaded server updates are queued and activate automatically
after confirmed shutdown, keeping the old client/server pair together until then.

## Cloud saves

The public game API owns saves keyed by `(player_id, app_id, slot)`. Identity
comes from the authenticated session, never a caller-supplied player ID.
The current browser `saves.load(slot)` returns JSON plus a version;
`saves.save(slot, data, version)` uses conditional writes and reports a conflict instead of
silently losing another device's update. Creation must also be conditional.

Bound per-slot size and per-player/app storage. The game owns
the save format and schema migrations; Infuser stores JSON with version metadata.
Retained versions for recovery are planned. Saves persist independently of game
container lifetime and build promotion. Native games can later opt into a
Steam Auto-Cloud-style file adapter; browsers need explicit API integration.

## Reactions

Players can celebrate each other's games with an oversized positive emoji and an
optional short comment. The interaction starts with choosing the emoji; only then
does the sentence field appear. Emoji-only reactions are complete reactions.

Start with a small, clearly labeled palette, for example: star (Amazing), thumbs
up (Nice!), rocket (Exciting), heart (Loved it), applause (Great job), and lightbulb
(Cool idea). The star is a reaction, not a numerical rating. Keep all choices
positive and equally prominent.

Use large text in a compact field, with a suggested starting limit of 140
characters. Friendly prompts such as "My favorite part was…" or "I loved when…"
can help someone begin; they are optional, not a required compliment formula.
Enter or the arrow button sends the reaction. Show the selected emoji prominently alongside
the player's name and sentence on the game page, retaining the large, brief feel.

Initial implementation should stay small: select an emoji, optionally write a
sentence, send, and allow the author to edit or remove their reaction. Character
limits and final palette can be adjusted when trying the UX with players. Store
reactions as platform data, independent of game containers, so they survive sleep
and updates. Reactions are separate from any future bug-report flow.

Do not prompt for reactions after play. Players choose when to react. The website
leads with friends currently playing and includes a clubhouse activity feed for
new games, updates and reactions. Only an adult admin can invite accounts; the
initial admin will be bootstrapped through a local operator command.

## Guest play

The dropdown beside Play includes **Play in guest mode** for sharing a device.
Each guest launch starts with an empty, temporary save namespace. It cannot read
or overwrite the signed-in player's cloud saves. Guest play ends without merging
its progress into the owner's account.

For browser games, a fresh game origin per guest launch also isolates localStorage,
IndexedDB and other browser-managed game files. Normal and guest launches must
not merely share a save path with a UI flag. Server-based games must use the guest
player identity for personal progress while joining the same shared multiplayer
session; guest mode does not create another world/server.

## Installation data

Domains, IPs, provider details, credentials and operator notes belong in private
configuration outside the repository. Public examples use reserved example names.
There is no provider-specific deployment dependency.
