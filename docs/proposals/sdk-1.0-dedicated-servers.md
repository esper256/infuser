# Infuser SDK 1.0 and dedicated game servers

Audience: Infuser developers.

**Direction accepted; detailed interfaces remain a proposal.** See the [SDK decisions](../sdk-1.0-decisions.md). This does not declare the existing SDK stable or change production behavior. It builds on the implementation at `11a24df` and the supplied Lanternfall feature request. Names in proposed API examples are illustrative until the wire contract is reviewed.

**Implementation update:** [managed Node packaging and hosting](../engines/node-server.md)
are now available and tested. Use that recipe's actual fields and endpoints.
Verified browser-to-server identity, dedicated session discovery and Join Friend
remain future work; their absence does not block publishing a Node/WebSocket game.

Infuser should give a game a dependable identity, storage, hosting and social environment while leaving its simulation and networking messages intact. An agent should be able to publish a browser client and a dedicated server together, without Docker expertise or an operator building an image. That publishing experience should also accommodate future Godot servers, Rust servers and other runtimes.

The recommendation is **one active deployment per game and private group, containing a matched client and server release, with game-owned gameplay sessions and Infuser-controlled access**. Start with a managed Node recipe and WebSocket adapter. Freeze the durable ownership and lifecycle rules before calling the SDK 1.0; engine recipes can keep evolving independently.

Existing foundations can be reused: constrained Docker execution, readiness and
graceful stop, on-demand leases, queued activation, scoped player/guest launches,
JSON and binary checkpoints, and SDK capability admission. The main additions are
managed packaging, process-scoped server credentials, verified connection lifetimes,
and a session/admission contract that works across hosting models.

## Decisions with the greatest long term impact

| Decision | Recommendation | Why changing it later would hurt |
| --- | --- | --- |
| Identity and ownership | Separate the player, launch, connection, gameplay session and server process. Scope every operation to the authenticated game and private group. | These identities become save keys, invitation targets, admission checks and recovery handles. |
| Game authority | The game owns simulation, room creation and final seat admission. Infuser owns identity, platform access and resource allocation. | Two authorities making independent room decisions produce races, phantom players and accidental worlds. |
| Social model | Default to one shared multiplayer session. Allow a game to explicitly support multiple sessions inside its one server. | A universal singleton excludes legitimate game designs; unrestricted room creation undermines the social goal. |
| Deployment | Activate the client artifacts, server image and compatibility declaration atomically. Keep one active server revision initially. | Mixed client/server versions can corrupt sessions and eventually persistent worlds. |
| Compatibility | Stable capability contracts coexist with existing preview and legacy behavior. A daemon update must not force games to republish. | SDK code is embedded in games whose agents may never return. |
| Admission | Discovery is advisory. Every connection gets fresh authorization and game-side admission to its exact target. | A presence snapshot cannot reserve a seat or grant access to a private room. |
| Persistence | Personal checkpoints and authoritative server data have separate ownership. Versioned code does not imply reversible data migrations. | Shared world state cannot safely be treated as an individual's cloud save. |
| Execution | Separate publisher credentials, build execution and runtime credentials. Author requests never become arbitrary Docker privileges. | A packaging shortcut must not give a game or dependency control of its host. |

## 1 Identity and private group boundaries

Use these concepts consistently in every adapter. IDs are opaque and must not be parsed for meaning.

| Concept | Meaning and lifetime |
| --- | --- |
| Player | A stable identity within this installation. Personal saves remain keyed to the existing player and game ownership. Names are mutable display text. |
| Guest | A distinct temporary player and save namespace for one guest launch, retained across its refreshes and removed on expiry. It never becomes its sponsoring account. |
| Launch | Authorization to run a particular game build. Opening a title screen does not allocate a backend or join a session. |
| Backend lease | A bounded claim that a caller needs a backend. It is not a gameplay room. |
| Runtime instance | One actual server process incarnation. A process restart creates a new instance ID, even on the same port with the same image. |
| Gameplay session | A particular shared activity: a co-op run, match, lobby or world. It has an opaque ID and an owner responsible for its lifecycle. |
| Connection | One authenticated transport connection. A reconnect creates a new connection; it need not represent a new player. |
| Join attempt | A short-lived, idempotent request to enter an exact session, optionally alongside an exact friend. It records success or a useful failure. |

Keep game modes, hosting authority, transport and persistence independent. A game
can support both single player and multiplayer; either can need an authoritative
server. WebSocket describes a transport, not a game mode. Persistent world data
does not imply an always-running simulation. Engine choice selects packaging and
adapter conveniences, not networking topology. Translate the existing
`persistent_world` mode through its compatibility adapter instead of forcing that
combined classification onto every future declaration.

Today `runtime.session_id` means a backend lease, while peer `session_id` means a gameplay session. Keep those existing meanings in compatibility adapters. Use explicit `lease_id`, `runtime_instance_id` and `session_id` in the new contract; never reinterpret the old field.

A game belongs to one private group today. Server credentials, room discovery, admission, invite redemption, logs and persistent storage must enforce that ownership. A game manifest cannot choose its group. Sharing the same app with another group later means a separate deployment and data namespace, not shared gameplay by accident.

Preserve existing player IDs and save ownership. Do not opportunistically replace IDs during SDK stabilization. Document their scope: portable save export or moving an account between installations requires an explicit future migration, not assuming an ID has global meaning.

A player may have several tabs or connections. Count connections, gameplay participants and spectators separately. A game chooses whether an additional connection replaces an existing one, is rejected, or occupies another seat. The SDK must not assume that an account equals a browser tab. Legacy shared-cookie leases remain supported; new lease operations have explicit ownership and cancellation handles so one tab cannot unintentionally release another tab's acquisition.

## 2 One server can contain several gameplay sessions

Preserve the original social default: ordinary multiplayer joins the existing shared session when possible. It does not create another one because the first is full, closed, updating or unreachable. The player receives the reason and stays on the title screen.

An explicit multiple-session capability allows games whose design needs separate runs. Lanternfall can opt in because its server already owns independent runs. Those runs share one container and resource allocation. Infuser does not require a container per room, nor does it invent another room system alongside the game's. One process is an initial scheduling policy, not part of the meaning of a session ID; future games can explicitly opt into distributed hosting without renaming their player or session identities.

Multiple sessions are an accepted relaxation of the earlier one-shared-world rule. Use UX to encourage joining friends while allowing an explicit independent run. Where supported, highlight Join friends and Play on my own on the game’s title screen; the Library’s primary Play still opens the game. Keep shared-session behavior as the default for games that do not opt in. Games with instances, private matches or simultaneous solo runs can then fit without changing Infuser's core concepts.

The game maps its local room objects to opaque platform session IDs. A room label such as `EMBER-417` is display text, not a password, identifier or access grant. Reusing that label must never make an expired invitation join a new run.

A process restart invalidates its transient sessions and join attempts. Future persistent worlds may have a separate stable `world_id`, but a world ID must not masquerade as a live session incarnation.

Peer-hosted games use the same social projection and targeting concepts. Their existing coordinator remains their admission authority, and their mandatory relay fallback remains unchanged. This design does not require rewriting existing PeerJS or Godot transports.

## 3 Keep session discovery separate from admission

The server reports a bounded directory of its sessions: ID, label, build, lifecycle, mode, connected participant handles, capacity and whether it currently accepts joins. Reports carry the runtime instance ID and a monotonically increasing sequence. Heartbeats expire stale reports; a delayed update cannot resurrect a deleted session or an old process's rooms.

Infuser derives identities from verified connection or participant handles. It rejects arbitrary player IDs, membership in another game/group, and reports from retired runtime credentials. A server can describe activities in its own game; it cannot fabricate another family's presence or grant itself additional platform permissions.

Directory updates are idempotent. Periodic full snapshots reconcile missed updates and deletions. Delta updates may come later without changing who owns the data. Bound the number and size of reports independently of simulation capacity.

A displayed room may fill or close before a player clicks. Infuser checks platform permission and target validity again; the game server atomically checks the actual room, target friend if supplied, invitation grant and available seat before admitting the connection. A friend leaving between discovery and connection must fail the targeted join. **The game has the final say on capacity and gameplay conditions.** Infuser still enforces its own connection ceiling and private-group boundary.

Do not introduce a second seat counter that independently claims to guarantee game admission. If future matchmaking needs central reservations, add a reservation protocol that the game explicitly participates in.

## 4 Make privacy and join policy separate

The request's `private: true` combines discovery and permission. Split those concerns:

| Property | Initial choices |
| --- | --- |
| Discovery | Listed to the private group, or unlisted |
| Admission | Group members, invitation required, or closed |
| Gameplay availability | Accepting players, full, between rounds, draining or ended |

An unlisted group-accessible room can be joined through an authorized friend target without appearing in a room browser. An invitation-only room requires a valid invitation; seeing a friend there is not an invitation.

Room links resolve an opaque, expiring invitation into a targeted launch intent after membership checks. They never grant access to another private group. Keep invitation credentials out of ordinary room listings and public metadata. Use an opaque invite route with token-redacted logs and a restrictive referrer policy, then exchange it for server-side launch state and remove it from the address bar. Short human codes are a separate, rate-limited lookup mechanism with expiry and attempt limits.

For Lanternfall, password-named rooms can initially map to unlisted, invitation-required sessions. If its authors want any group member to Join Friend without a code, that is an explicit group-accessible policy. Do not silently turn their password into a public room label.

Ordinary Play still opens the game's title screen. Join Friend and Join Room are targeted intentions, never instructions to create a replacement session. A failed target returns a useful error; the game may offer a separate, explicit Start action.

## 5 Authenticate once without redesigning gameplay packets

Provide a transport-independent identity context and a small server adapter. A verified context includes the player, guest status, launch, client build, runtime instance, connection and optional targeted join attempt. The server never trusts a player ID or display name from a game's ordinary JSON message.

Each server process receives a short-lived, renewable credential scoped to its
group, game, build and runtime instance. It can verify admissions and report its
own sessions; it cannot publish builds, administer members, inspect another game,
or read arbitrary personal saves. Infuser injects this credential automatically,
keeps it out of artifacts, and revokes it when the process is retired. A controller
restart reconciles surviving containers and their existing incarnations; it does
not invent fresh room identities for a process that never stopped.

For the first managed WebSocket adapter:

1. The client requests a connection or targeted join attempt using its launch authentication.
2. Infuser checks group, build, runtime incarnation, invitation and target policy. This does not yet promise a seat.
3. The browser opens the supplied same-origin WSS endpoint. Its launch cookie authenticates the upgrade; the URL's attempt ID is a lookup handle, not a bearer credential.
4. The gateway strips caller-supplied identity headers and forwards verified context to the intended backend. The server adapter validates the connection proof and binds it to that process and attempt.
5. The adapter calls the game's admission callback. The game reserves its seat atomically, completes the upgrade, and binds the authenticated connection to its room. Failure releases the reservation.
6. The adapter records the attempt outcome and membership idempotently. Ordinary WebSocket messages then use the game's existing protocol, without an Infuser envelope around every message.

The browser helper returns a normal WebSocket after admission. Its error path queries the join-attempt result, because browser WebSocket failures alone do not supply a useful HTTP rejection body. A lost acknowledgement is recovered using the same attempt ID; it must not join twice. Reservation expiry and disconnect handling release abandoned seats.

A server adapter should make this integration a small boundary change around an existing `ws` upgrade handler and room join function. It should not replace Lanternfall's JSON messages, 30 Hz simulation, or snapshots. Games with admission information that only exists in their initial gameplay message can use a bounded admission phase before publishing membership; the adapter still prevents unverified identities and misleading Join success.

Web backends stay on private internal bridges or loopback-published ports. Games with direct TCP/UDP endpoints use short-lived, narrowly scoped connection tickets and the same identity/admission semantics through an adapter. The existing verification route remains available. Do not claim native multiplayer support for SDK 1.0 until a real native game exercises that path.

An IP allowlist remains defense in depth. It is neither a player identity nor a private-group boundary.

## 6 Own the lifetime of authenticated connections

Authentication at the HTTP upgrade alone is insufficient to promise prompt logout, revocation or expiry. The managed WebSocket gateway must know which live connections belong to each authorization and be able to close them.

Keep Caddy for TLS and ordinary routing. Route the new managed-connection endpoint through a Rust gateway owned by Infuser, which manages connection lifetimes and forwards gameplay frames without interpreting them. Existing legacy server proxy routes keep their behavior. Caddy itself transitions WebSockets into a tunnel, and configuration reloads have connection-lifetime behavior that needs testing; a forward-auth check is not a continuing revocation mechanism. [Caddy reverse proxy documentation](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy#streaming).

Use bounded message sizes, output queues, handshake times and idle connection checks. Do not make Lanternfall's 64 KiB message size a platform-wide constant. Publish effective limits, and let games request values within operator policy.

Define revocation scope explicitly: disabling a member or withdrawing game access
ends all affected launches, including sponsored guests; leaving a game closes that
launch's connections; signing out ends launches associated with that browser login
session, without logging out other devices. This requires tracking the parent login
session rather than treating a game ticket as independently renewable forever.
Legacy behavior remains behind its existing adapter until the new contract is
adopted.

Membership revocation closes affected connections within a documented bound. On control-plane failure, deny new admission and allow existing gameplay only through a bounded renewal grace. Never silently switch to public matchmaking. A controller or gateway restart may disconnect transports; a surviving game process can support explicit reconnect and state resynchronization during a grace period. Do not promise uninterrupted sockets or continued play after the process itself dies.

These semantics should be tested before the capability is marked stable. The exact grace and renewal intervals are advertised policy values, not hardcoded assumptions in game code.

## 7 Separate backend demand from browser presence

A title screen can read presence and save data while its backend is asleep. A single-player run may still require a backend. That is already the right model for Lanternfall and should remain explicit in the manifest.

Backend lifetime depends on bounded acquisitions, authenticated gameplay connections, reconnect reservations and short shutdown work. A server's own heartbeat or a stale room report cannot keep an unused container alive forever. Browser heartbeat throttling must not stop a server that still has valid gameplay connections and server-confirmed membership. That connection renews both its bounded backend demand and its associated launch authorization, subject to membership and expiry policy, so a backgrounded guest does not suddenly inherit an empty save namespace.

When effective demand reaches zero, start the idle timer. Keep reconnection grace separate from idle shutdown. Release is idempotent, and a delayed release cannot cancel a newer acquisition. A cancelled acquisition also needs a tombstone or generation check so a delayed retry cannot warm the game again.

Presence is a projection with separate facts:

- Online in the private group.
- Game open, including its title screen.
- In a gameplay session, including a server-backed single-player run.
- Participating in multiplayer.
- Eligible for the viewer to join.

A server connection alone does not mean multiplayer. Likewise, a multiplayer session waiting for its second player is still multiplayer. The game declares the activity mode; occupancy does not infer it.

Reading presence starts nothing and renews nothing. Social widgets do not alter fullscreen. Children see a small connection/status indication; detailed latency, capacity and failure diagnostics belong in adult and publisher tools.

## 8 Publish immutable releases and drain deployments

A release binds the web/native client artifacts, server image digest, target architecture, packaging recipe, requirements and gameplay protocol declaration. Installation resolves these into a deployment. Activation changes the complete pair atomically.

The first implementation requires exact client/server build pairing. Recording a game-owned gameplay protocol version now leaves room for explicitly compatible client-only updates later. Do not implement concurrent server revisions, rolling pools or automatic protocol negotiation for this first version.

Publication uploads and validates immediately. Packaging happens before activation, never when a child presses Play. An active deployment can keep running while the new release waits.

When an update is queued:

1. Mark the deployment draining and send a durable, repeatable lifecycle notice through the server control channel.
2. Stop creating new sessions. Existing participants and bounded reconnects can continue. Any permitted new joins to existing sessions must obey an explicit drain policy.
3. New launches may receive the current pair until the switch, but cannot start a new run that prolongs the drain. Show “Update finishing” and preserve title-screen state.
4. Once sessions and shutdown work finish, gracefully stop the process, confirm its exit, and activate the complete new pair under one serialized transition.
5. An old title screen cannot connect to the new server. Return `build_changed` with a reopen action.

Use a server-initiated control channel with acknowledged command IDs and recoverable state, rather than inventing an arbitrary `/drain` callback path for each engine. Polling is a valid initial implementation. `SIGTERM` and a bounded stop timeout remain the final shutdown mechanism even when the adapter misbehaves.

Normal updates should not impose an arbitrary deadline that destroys live runs. The adult can inspect a stuck drain and choose Force restart, with its consequences shown. A host may configure a maximum drain duration, but the configured forced-stop policy must be explicit. Superseded unpublished releases can be discarded without changing the active run; an operator can cancel a drain that has not reached its irreversible stop step.

A crash invalidates transient session targets. Restart only with continuing demand, bounded backoff and a retry ceiling. Existing clients get an explicit interrupted-run result. A Join Friend retry must not create a replacement world. Persistent data survives if declared; unsaved in-memory runs do not.

## 9 Personal saves and server data have different authority

Keep the existing JSON/binary checkpoint API and its player/game/guest ownership. Conflicts preserve both candidates. Formats, limits, idempotency, history and failure behavior need stable semantics, not merely stable method names.

Server-authoritative progression belongs in server-owned persistent storage by default. Do not give every server token unrestricted access to every player's cloud saves. If a future game needs the server to commit personal checkpoints, add an explicit delegated scope tied to a verified participant and save group. This also establishes the right authority boundary for future achievements.

A persistent volume belongs to the game deployment's group and app, not a particular build or container incarnation. Reserve space for a future explicit world namespace. The game owns database schemas and migrations; Infuser owns mounting, retention, backup hooks and declared shutdown coordination.

A game update that changes persistent data needs a backup/recovery point and migration policy. Code rollback alone cannot undo the change. Automatic fallback to an older server is permitted only when its data compatibility is established. A failed readiness check after a migration must not blindly start an old image against new data.

Lanternfall can declare ephemeral server data. The generic lifecycle must still leave room for persistent games, without forcing a database or a cloud-save API onto its throwaway runs. Guest isolation applies to personal progress; participating as a guest in a shared world can still affect that shared world through ordinary gameplay.

## 10 Managed packaging is a service with recipes

The existing agent remains responsible for the game's development and deciding when to publish. Infuser packaging does not become a GitHub deployment watcher or take over source editing.

A publication contains client artifacts plus one server input: a supported source recipe or an approved prebuilt image. The first recipe is plain Node JavaScript with a lockfile. Keep operator-staged digest-pinned images working as the advanced route; additional recipes can later support compiled Godot and Rust servers without changing session APIs.

The agent uploads the server context, not its entire repository or machine's `node_modules`. Validate paths and exclude credentials, `.git`, environment files and unrelated assets. Dependencies are installed for the target host architecture, so an author publishing from a Mac does not accidentally ship Mac native modules to a Linux server.

For the initial Node recipe, use a maintained, operator-supported Node line and resolve its base image to an exact digest. Install production dependencies from the lockfile with lifecycle scripts disabled. This fits plain JavaScript dependencies; packages needing native compilation or install scripts need an explicitly supported recipe, not a silent weakening of the default. npm documents that `ignore-scripts` disables package scripts during installation, while an explicitly invoked `npm run` still executes its requested script. [npm ci documentation](https://docs.npmjs.com/cli/v11/commands/npm-ci/#ignore-scripts).

Separate dependency fetching from execution: allow the package fetch stage access only to configured registries or a dependency proxy, then assemble and test with restricted networking. Do not accept arbitrary project npm configuration, Git dependencies or remote URLs as a way around that policy. Record the lockfile, runtime image and recipe digests. Digest pinning stabilizes an artifact; it does not prove its code is trustworthy or guarantee that two independent builds are byte-identical.

Use a separate, constrained build worker, preferably rootless, with no production Docker socket, runtime secrets, OAuth keys, cloud credentials or game volumes mounted inside the job. A narrowly scoped controller imports the finished image. Docker warns that control of its privileged daemon permits powerful host access. Rootless BuildKit exists, but its network isolation needs explicit configuration; “rootless” alone is not the policy. [Docker security](https://docs.docker.com/engine/security/#docker-daemon-attack-surface), [BuildKit rootless mode](https://github.com/moby/buildkit/blob/master/docs/rootless.md).

One bounded packaging job at a time is sufficient initially. Queue jobs when safe capacity is unavailable. Runtime CPU/RAM reservations, build limits, disk reserve, cache retention and log limits protect the whole installation; this does not add per-family resource budgets.

Promote only after image validation and a bounded readiness smoke test in a disposable sandbox. Persistent-volume smoke tests must not touch the live world. Retain active and rollback images; prune failed and superseded artifacts within a cache budget.

Expose a durable build job with states such as queued, packaging, checking, ready and failed, plus cursor-based bounded logs and an actionable error code. Retrying an upload or finalization with the same request ID must recover its existing result, not create extra builds or activate twice.

## 11 Keep runtime settings separate from game design

The container listens on the assigned `PORT`, with the legacy `INFUSER_PORT` alias
preserved. It gets bounded writable temporary space, a declared persistent data
mount only when requested, automatically injected server authentication, and a
SIGTERM stop deadline. The adapter reads these settings; the child manages none of
them. Readiness means the service can accept connections, not that it currently
accepts new gameplay sessions. A draining server remains healthy while finishing
its runs.

The author-facing file should ask for only what cannot be inferred reliably. This is an illustrative declaration, not a manifest accepted by the current publisher:

```json
{
  "sdk": {
    "major": 1,
    "api_major": 1,
    "required_capabilities": [
      "identity.v1", "runtime_sessions.v1",
      "sessions.v1", "connections.websocket.v1"
    ]
  },
  "play_modes": ["solo", "multiplayer"],
  "solo_runtime": "shared_server",
  "server_start": "on_demand",
  "server": {
    "package": {"recipe": "node", "context": "server", "entry": "server.js"},
    "endpoint": {"transport": "websocket", "health": "/healthz"},
    "storage": "ephemeral"
  },
  "sessions": {"policy": "multiple", "max_players_per_session": 8},
  "launch_intents": ["join_friend", "join_session"]
}
```

`solo` is the existing wire value; the UI continues to say Single player. The CLI can suggest a runtime line from the project's engine requirement, but the server must return the resolved runtime and reject unsupported requirements rather than silently changing them. The exact new manifest field layout should be decided alongside its validation examples.

Distinguish a process's total connection limit, each session's participant capacity, the directory's maximum session count and CPU/RAM allocations. Lanternfall's requested 30 rooms of eight players is not evidence that its requested resource allocation can run 240 players at 30 Hz. Its effective capacity needs measurement.

Provide reasonable host defaults and allow explicit requests within policy. Return effective limits and explain reductions or rejection; do not silently advertise capacities the host cannot support. Never expose arbitrary Docker flags, host mounts or privileged networking through the game manifest.

## 12 The smallest useful SDK surface

| Surface | Infuser provides | The game provides |
| --- | --- | --- |
| Client core | Hosted/standalone detection, verified context, lifecycle renewal, capability lookup, typed errors | Its title screen, mode choice, connection/recovery UI |
| Backend lifecycle | Acquire, status, cancellation, release, readiness and effective limits | When a server is needed |
| Sessions and presence | Discovery, scoped join attempts, invitations, friend targets and membership projection | Session creation, activity mode, labels, availability and atomic admission |
| WebSocket adapter | Verified connection context and lifecycle tracking without changing gameplay frames | Existing serialization, simulation, snapshots and resynchronization |
| Server SDK | Scoped process registration, reports, admission binding, renewals and lifecycle notices | Room storage, seat rules, draining and graceful shutdown |
| Saves | Conditional personal checkpoints with JSON/binary data, history and preserved conflicts | Serialization, schema migration, explicit conflict choice |
| Publisher CLI and API | Upload, recipe packaging, validation, immutable release and activation state | Built client assets, server input, publication timing |

Offer the first server helper as a small Node package for the `ws` integration, with a language-neutral HTTP control contract underneath. Rust remains the controller implementation. Future Godot/Rust server libraries implement the same contract.

Representative client flow:

```js
const context = await infuser.ready;
const intent = await infuser.launchIntent.consume();
// Present the game's title screen; only proceed on the player's choice or intent.
const backend = await infuser.runtime.acquire();
// Wait for readiness with Cancel and a bounded deadline.
const socket = await infuser.sessions.connect({
  backend,
  target: intent?.target ?? selectedSession,
  requestId: crypto.randomUUID()
});
// Keep the game's existing WebSocket messages and simulation protocol.
```

This sketch omits failure handling for readability. A concrete implementation must distinguish “join this exact session” from an explicit “create/start a session” request. An absent target must not silently create a parallel room. Existing `launchIntent.consume()` remains at-most-once: recoverable admission attempts solve network retries after consumption, without pretending a lost consume response can be replayed safely.

## 13 Compatibility rules for SDK 1.0

Freeze semantics for a supported capability, independently of daemon release cadence. Package versions identify SDK implementations; capability versions identify behavior. New optional fields are additive. Unknown required capabilities fail admission before publication or launch. Optional features can be absent without breaking the game.

Existing unversioned and preview-v1 imports keep their meanings. In particular, do not change lease IDs into room IDs, singular peer presence into an arbitrary room selection, JSON slot ownership, guest save scope, or default launch-time warming for old builds. Add new session APIs and capability flags beside those contracts. The old singular presence field can continue describing its original peer session; new consumers use the explicit session directory.

Normalize new failures through a typed SDK error containing a stable code, displayable message, retry guidance and optional delay. Do not require games to parse prose or infer everything from HTTP status. Preserve old wire envelopes through their adapters. Codes include `session_ended`, `friend_left`, `session_full`, `invite_required`, `build_changed`, `server_draining`, `capacity_unavailable`, `identity_expired` and `platform_unreachable`.

Publish deadlines, renewal intervals, size limits and capacity bounds through capabilities/context. Paginate directories and logs with opaque cursors. Keep reports and mutation payloads bounded. Gameplay packets remain outside the platform's JSON schema.

An SDK 1.0 release does not require freezing every future capability. Stabilize identity, personal saves, backend lifecycle and the tested dedicated WebSocket/session contracts. Keep native multiplayer, additional engine recipes, matchmaking and achievements explicitly outside that guarantee until separately exercised. Existing preview games retain their compatibility path.

Hosted detection must distinguish absent platform from broken platform. When Infuser is absent, an existing game may use its normal standalone backend and save behavior. Once a valid hosted launch is established, authentication or network failure is a hosted error; it must never fall through into public matchmaking, another player's save namespace or a different world.

## 14 Local development and operational visibility

Provide `infuser dev` using the same gateway, control protocol and server adapter as production, with explicit development identities and disposable state. Keep those identities and credentials impossible to admit on a production host. Avoid a separate mock that accepts behavior production rejects.

A Node game should run with its ordinary local Node process; Docker is optional for day-to-day editing. Publishing can do target-architecture packaging remotely. A container acceptance command checks the additional filesystem, network and resource constraints before promotion. Local development success is not proof of production isolation compatibility.

Publisher diagnostics should identify the deployment and runtime incarnation, readiness, crash history, exit/OOM status, active sessions, resource limits and recent bounded stdout/stderr. They are scoped to the authorized game and group. Show metrics to adults and agents, not in children's gameplay overlays.

Redact known credentials and avoid logging transport assertions or invitation secrets. Do not promise arbitrary application logs are universally secret-free: a game can print its own data. Log access remains private and rate-limited.

## 15 Delivery order and acceptance

Deliver a narrow dedicated-server path first, then freeze the proven contract. Do not wait for every engine or a multi-host scheduler.

| Step | Deliverable | Proof required |
| --- | --- | --- |
| 1 | Identity vocabulary, trust scopes, exact-pair releases and error/admission contract | Review concrete schemas and old-client behavior; settle the shared versus multiple session default. |
| 2 | One-command Node packaging with immutable image pairing | Publish/update from a machine without Docker; packaging failure leaves the current game untouched; verify target architecture and constrained execution. |
| 3 | Authenticated managed WebSocket adapter with single shared session | Two accounts and a guest; solo requiring a backend; forged context rejected; title screen stays cold; actual connections keep a backgrounded player's run alive. |
| 4 | Generic session directory, targeted joins, invitations and optional multiple sessions | Last-seat race, closed/replaced room, friend departure, private-room access, two rooms in one process, cross-group denial and no phantom presence. |
| 5 | Drain, crash recovery, logs and adult restart | Active run finishes across a game publication; new runs cannot prolong drain; container crash changes incarnation; controller restart reconnects without inventing a new world. |
| 6 | SDK 1.0 candidate and compatibility freeze | Unchanged shipped games pass; Lanternfall plus a small game with a different admission flow exercise the same contract. |

Keep one fast fake-clock lifecycle suite, one real HTTP/WebSocket journey, the existing frozen old-client/save journeys, and a focused Docker packaging/start/drain journey. Add a tiny persistent-state fixture for migration/rollback behavior before promising that capability. These prove the boundaries without building another full showcase game or making every change wait on a large CI matrix.

Before declaring SDK 1.0, explicitly exercise lost responses, duplicate requests, reordered reports, daemon restart, connection revocation, background tabs, guest expiry, resource exhaustion, archive traversal, and a failed image activation. Those failures are where superficially similar game integrations diverge.

For Lanternfall specifically, the adaptation should be limited to server packaging, verified identity/admission at the WebSocket boundary, mapping its existing rooms into session reports, consuming targeted joins, and handling drain/reconnect. Keep its gameplay code and packet protocol. Its claimed dependency/runtime compatibility and capacity still need validation against the actual project.

## What changes from the supplied request

| Request | Proposed platform decision |
| --- | --- |
| Automatic Node server packaging | Adopt as the first recipe behind a generic packaging service. |
| One container with game-owned rooms | Adopt, with explicit multiple-session opt-in and the shared-session default preserved. |
| Server reports arbitrary player ID arrays | Report verified connection/participant handles; Infuser derives visible identities. |
| `private: true` allows code, link or Join Friend | Separate discovery from permission; invitation-only does not become accessible through friend presence. |
| Password or label in the room URL | Use an expiring opaque invitation bound to the exact session incarnation. |
| Trusted plain identity headers | Expose a verified SDK context, with gateway-only ingress and instance-bound proof; header names are an adapter detail. |
| Presence check admits the player | Treat discovery as advisory; the game atomically admits the final seat. |
| `/drain` callback | Use a shared, acknowledged server lifecycle protocol, with SIGTERM as the final fallback. |
| Thirty rooms as declared capacity | Separate directory limits from verified simulation capacity and host connection limits. |
| Lightweight fake local platform | Reuse the production protocol and adapters in an explicit development mode. |

The core product choice is to keep Infuser responsible for **who may connect, what release they run, and how their progress survives**, while games remain responsible for **what their sessions mean and what happens inside them**. That supports Lanternfall immediately without making its room structure the shape of every future game.
