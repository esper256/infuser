# Product boundaries

Audience: Infuser developers.

Infuser borrows familiar Steam concepts without implementing Steamworks.
**Infuser** is the website and hosting platform; **Infuser Play** is the native
player application. The game SDK is [pre-1.0](sdk-contract.md).

| Concept | Meaning |
| --- | --- |
| App | A game's stable identity, ownership, and saves |
| Build | An immutable set of validated client artifacts and optional server settings |
| Active build | The build used for new launches; an update can wait for multiplayer to end |
| Session | One gameplay room within a game |
| Presence | Separate online, game-open, and multiplayer states |
| Cloud save | Player/app-scoped JSON slots or declared checkpoint groups |

## Components

- **Rust daemon:** public website/publishing/game APIs, SQLite storage, session
  admission, resource budgets, and Docker game-server lifecycle. Privileged
  controller routes are loopback-only and use an operator credential.
- **Website:** Google login for invited members, Library, presence, reactions,
  chat, guest play, and administration. Only admins invite or disable members.
- **Publishing CLI/API:** agents connect once, upload runnable artifacts, validate,
  and publish updates. They retain control of building and deciding when to publish.
  A published web build also keeps gzip and Brotli copies of its large files so
  players download less. The original files stay available.
- **Game SDK/adapters:** browser JavaScript, Godot previews, and a PeerJS preview.
  [Engine recipes](agents.md#choose-the-integration) state tested capabilities.
- **Infuser Play:** downloads, verifies, updates, and launches supported native
  games, with browser login and isolated guest saves.

Caddy handles HTTPS and routing; the login proxy handles Google OAuth. Optional
shared services provide coturn relay and PeerServer signaling. PeerServer runs
once for the installation, not once per game. Static browser games need no game
container. Operators configure services; game authors declare their requirements.

## Runtime rules

`play_modes` can include single player, multiplayer, or both. Engine choice does
not imply a topology. [Publisher capabilities](public-api.md) are authoritative
for the installation. A valid manifest alone does not prove runtime availability.

Peer-hosted WebRTC uses Infuser admission and TURN fallback. LAN/direct internet
connections are preferred when ICE can establish them; forced-relay play must
also work. Raw peer TCP/UDP, LAN-only networking, and the separate `relayed`
datagram/WebSocket mode are unsupported. Host departure ends the room; seamless
host migration is not implemented.

Dedicated and persistent-world games use one Docker server per app, within CPU,
RAM, disk, and port budgets. Sleeping games retain their port but no running game
process. Persistent volumes survive shutdown; the game must checkpoint before
its shutdown deadline. Persistent world does not promise an always-running
simulation. See [on-demand leases](runtime-sessions.md).

Peer-hosted games can run multiple independent rooms, each with its own host. A dedicated server can declare a shared
session or multiple explicit runs inside its single container; see
[dedicated sessions](dedicated-sessions.md). A failed/full/closed Join friend never
creates another world. Ordinary Play opens the title screen; [Join friend](launch-intents.md)
is an optional explicit action using the game's reported joinability.
Games own their mode selection, waiting screens, and any spectating.

Updates upload immediately, then wait for the current server or all live peer rooms to stop
before activation. Existing browser launches retain their selected artifacts.
Old clients must reopen before entering a newer build's multiplayer session.
Preview is a visibility setting, not a parallel branch or a second server.
A binary rollback cannot undo a game-owned save/world migration.

## Identity and storage

Games run at `/` on isolated HTTPS hostnames. A launch exchanges a one-use ticket
for a scoped game session; the website's Google cookies stay on the website.
Games never receive publisher or operator credentials. Direct server protocols
must validate scoped player tickets through `/api/server/verify`.

[Cloud saves](cloud-save-design.md) derive ownership from the authenticated launch,
not a supplied player name. Conditional writes preserve conflicts; games own the
JSON schema and migrations. Godot's web/native adapters use these APIs. Arbitrary
binary filesystem synchronization is not implemented.

Guest play starts with empty temporary saves and a separate browser origin or
native save directory. It never copies or merges the owner's progress. Guest
multiplayer uses its own identity and follows the game's session policy.

Titles, descriptions, covers, tags, reactions, and chat are platform data, separate
from server lifetime. [Metadata edits](game-metadata.md) do not replace game builds.
Installation domains, IPs, credentials, and operator notes belong in private
configuration, never public repository examples.
