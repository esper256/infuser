# SDK 1.0: stabilization plan

Audience: Infuser developers.

Status: proposed release boundary and implementation order, following the
[accepted decisions](../sdk-1.0-decisions.md). No capability becomes stable by
merging this document. The shipped SDK still advertises `preview`.

Infuser should be able to update without asking every game author to republish.
Freeze the behavior games depend on, then keep improving its implementation.
Do not freeze the website, daemon internals, or every experimental adapter together.

## 1. Promise compatibility per capability

Keep manifest schema, SDK packaging major, wire API major, and feature capability
versions separate. Their existing names and paths stay intact. A `.v1` capability
currently means availability, not stability; changing that interpretation would
mislead existing games.

Add an optional capability-details map to the existing advertisement, with a
reviewed stability value for each capability. Preserve the existing string list,
global preview status, and admission behavior while this is being built. Document
the exact additional shape before implementing it. Do not change an existing
required capability into a new name just to mark it stable.

The eventual SDK 1.0 release should identify its stable capabilities explicitly.
Experimental adapters can ship alongside them with an explicit preview label.
An installation advertises only features it can actually provide; a stability
label does not substitute for TURN, a managed backend, or a game's opt-in manifest.

Within a stable contract, preserve ownership, method signatures, response shapes,
error outcomes, and retry meaning. Optional additions must work with unchanged
clients. A new required field, enum value that changes an existing decision, or
different interpretation of an ID is a compatibility change, even if JSON parses.
Use a new capability version or API major for incompatible behavior and keep the
old adapter. Sharing implementation is fine; sharing a handler is not permission
to change both contracts accidentally.

Recommend no calendar-based retirement for 1.x. Before any future removal, inventory
stored builds that require it and offer an explicit migration. Preserve legacy
and preview integrations too; their users should not pay for the stabilization work.
Security or dependency failures may require an explicit unavailable state, never
silent substitution of a new contract or save namespace.

## 2. Keep identity and data ownership independent of runtime lifetime

| Identity | Meaning that must not change |
| --- | --- |
| Player | Opaque authenticated identity; names and emails are not save keys |
| Guest | Separate identity and temporary personal progress for one launch; refresh preserves it, a new launch starts empty |
| Launch | One authorized play context; it can exist at the title screen without a server or multiplayer room |
| Runtime lease | Demand for a managed backend; the existing `runtime.session_id` is a lease ID, not a gameplay session |
| Process instance | One incarnation of the game's managed server, bound to its build and private group |
| Gameplay session | One run inside a peer room or managed process; an ended session ID must not identify a replacement run |
| Connection attempt | One recoverable admission attempt, not a player, room, or bearer credential |

Authorization always includes the authenticated installation/group/game context.
Do not promise that IDs can correlate accounts across installations, or alter
existing save keys to implement that idea. Cross-group presence and game access
remain forbidden; operator inspection does not create a participating player.

Separate personal slots/checkpoints from persistent server worlds. A personal save
survives releases. A persistent world belongs to the deployment, not the container
incarnation. The game decides which authoritative world changes survive a guest's
participation; guest isolation does not mean rolling back a shared world.

Preserve both save APIs. Slots retain their existing compare-and-swap behavior;
checkpoints retain complete-group snapshots, immutable revisions, idempotent commit
IDs, and stored conflict candidates. Never migrate slot data implicitly. An omitted
checkpoint file means absent, not unchanged. Conflicts and failed writes must not
discard pending progress. History retention is bounded; a successful old receipt
does not promise its revision remains available forever.

Games own save schemas and migration. Infuser owns storage, access, and coherent
commit boundaries. Do not run arbitrary game migrations inside the controller.
Offline durability, cross-installation saves, and a generic merge engine are outside
1.0. See the implemented [checkpoint contract](../cloud-save-design.md).

## 3. Keep game rules out of the hosting protocol

One managed process may host one shared multiplayer run or multiple explicitly
created runs. Keep the accepted `shared` and `multiple` policies. This does not
require a container per run or a fleet scheduler. Independent server-backed single
player is also valid; browser-only single player should keep working without a
server. A later release can add multiplayer to an existing single-player app.

Infuser verifies identity, warms the process, mediates transport, reports discovery,
and manages demand. The game owns simulation, room selection, atomic seat admission,
and final readiness. Presence is a hint, never a reservation or access grant.

Ordinary Play opens the title screen. An ordinary Multiplayer choice can join an
available run; it does not create another merely because existing runs are full.
Separate runs require an explicit choice. Join Friend and resume must either reach
the exact target or fail visibly. No fallback to an untargeted join.

Keep gameplay frames unchanged. The Node/WebSocket adapter is one implementation
of the hosting contract, not a requirement that every future engine use Node or
WebSockets. Future Godot servers, UDP transports, or other networking libraries
need explicit adapters and capability review, not imitation of browser APIs.

Peers keep LAN/direct connectivity as optimizations and mandatory TURN fallback.
Peer host departure ends the run; host migration remains a later capability.
Standalone mode is selected only when hosted configuration is absent. An invalid
hosted launch cannot silently switch matchmaking services or save ownership.

## 4. Preserve matched releases and bounded lifetimes

Keep immutable client/server releases and exact pairing. Uploading an update can
finish while the old build runs; activation waits for its sessions and process to
finish. Draining stops new runs/joins while allowing existing play and bounded
resume. An old title screen must reopen instead of connecting to an incompatible
new backend. Do not introduce rolling server pools or protocol negotiation in 1.0.

Process idle shutdown preserves declared persistent storage, not continuous world
simulation. Reports and presence reads do not renew demand. Startup leases and
disconnected reservations expire; authenticated live transports can keep a
backgrounded game alive. Website logout remains independent of ongoing play;
membership removal still revokes access.

Treat timeouts, retention, and capacity as advertised limits where available, not
universal hardcoded values. Record which current values are protocol constraints
versus operator tuning before freezing them. A stopped process cannot promise
in-memory resume. A controller restart can allow bounded resume only while the
same game process and reservation survive.

Code rollback does not reverse a game database migration. Retain the current
backup/recovery boundary and document this limit; automatic world-schema rollback
is not part of SDK 1.0.

## 5. Specify failure behavior, not just happy-path JSON

Keep existing wire error envelopes. Slot/runtime routes use string errors; newer
checkpoint/dedicated routes have structured codes. Replacing every envelope for
visual consistency would break clients. Add machine-readable information only
through a compatible, tested extension. Unknown codes need a safe visible fallback.

| Operation | Required retry and failure behavior |
| --- | --- |
| Checkpoint commit | Retry identical ID and body; recover the original result, including conflict. Never invent a new ID after an uncertain write |
| Slot save | A stale version fails; reload and resolve. Do not pretend this has checkpoint receipts |
| Backend acquisition | Repeated acquisition adopts the live lease; recover status after uncertainty. A delayed release cannot end a newer lease |
| Dedicated preparation | Same attempt ID and body recover one result within its lifetime/coordinator; cancel or consume cannot open a second connection |
| Server room reports | Full sequenced snapshots; duplicate reports do not renew freshness; re-register after coordinator change |
| Join intent consumption | Durable at-most-once delivery, not guaranteed delivery. A lost response returns the player to the title screen |
| Exact join/resume | Keep the target on every attempt; ended/full/departed/draining outcomes never create a replacement world |
| Cancel/startup timeout | Bound retained work even if cleanup cannot reach the server; no eventual surprise entry into a game |

Do not add a universal retry wrapper around these APIs. Intent consumption and
idempotent commits deliberately have different semantics. Helpers should expose
cancellation and usable errors without making the game implement platform renewal.

## 6. Freeze a small, evidence-backed surface

The initial review covers these existing capabilities; this table is a proposed
release scope, not an assertion that they have passed the freeze gate.

| Candidate | Evidence and remaining gate |
| --- | --- |
| `identity.v1`, `saves.v1` | Frozen legacy journey exists. Freeze a versioned client too; preserve private-group/guest and old-client behavior |
| `cloud_checkpoints.v1` | JSON/binary and conflict journeys exist. Main-import binary helpers now pass a real HTTP save/reload journey; freeze that surface with the candidate client |
| `runtime_sessions.v1`, `sessions.v1`, `connections.websocket.v1` | Generic shared/multiple-run, browser, and ARM64 Docker/Caddy journeys passed. Record real Lanternfall integration/playtest evidence and freeze copied Node helpers |
| `presence.v1`, `launch_intents.v1` | Peer and dedicated journeys exist. Freeze stale-target, hidden-game/group isolation, at-most-once, and no-backend-on-read behavior |
| `peers.webrtc.v1` | Forced relay and browser studies exist. Complete physical sleep, full-duration renewal, and supported-browser acceptance before marking adapter stable |
| `peers.peerjs.v1` | Independent fixture passed. Keep adapter preview until an existing user's game confirms the integration |

Godot web exercises the same core wire contract, but its adapter needs its own
support statement and browser matrix. Native Infuser Play remains a separate
single-player preview pending installed login handoff and physical Windows testing.
Stable cloud APIs would not automatically make every native integration stable.
Achievements, native multiplayer, additional engines, and large-world storage APIs
remain outside this freeze. None needs to block unchanged working games.

## Implementation order

1. **Close the entry-point gap — implemented.** The core SDK now exposes the
   synchronous `encodeBinary`/`decodeBinary` helpers already implemented in the cloud
   module, matching the guide. Existing async methods and the standalone module
   retain their behavior. The real HTTP compatibility journey loads both served
   modules and checks mixed JSON/binary commit/reload, idempotent retry, invalid
   binary data, and the unchanged legacy client's behavior.
2. **Record the candidate contract.** Create a compact developer contract inventory
   of public exports, request/response examples, null/absent behavior, identity scope,
   error codes, limits, and operation-specific retries. Link the existing integration
   pages rather than making authors read another design document. Keep actual runtime
   validators authoritative; use shared schema definitions where they remove real
   duplication. Do not begin a whole-API code-generation rewrite.
3. **Freeze candidate clients and journeys.** Keep the existing legacy fixture
   immutable. Add a separately identified versioned browser client and copied Node
   adapter fixture, including lazy imports. The current versioned compatibility
   test downloads today's SDK, so it cannot detect all breaks to uploaded adapters.
   Execute both frozen and current clients against the daemon. Exercise the failure
   table above, especially cancellation, lost responses and cross-build/group cases.
4. **Expose reviewed stability information.** Add the optional advertisement metadata
   after its shape is documented and old consumers are exercised. All entries remain
   preview initially. Reject unsupported required features as today; optional missing
   features must not stop launch. Do not equate metadata with acceptance evidence.
5. **Close external acceptance and declare scope.** Collect actual game and device
   results, select the capabilities that passed, publish their support matrix and
   exact contract snapshot, then declare that scope stable. Anything unverified
   stays explicitly preview. Do not make up Lanternfall or Windows test results.

Keep fast semantic checks in the existing test workflow. Run Docker/Caddy, physical
devices, sleep, and long renewal journeys when the affected boundary changes and
before the relevant freeze; do not put them in every PR. No new approval machinery.

The [publication capture](../publication-capture.md) archive remains useful for
replaying real importer requests with fresh test credentials. It cannot replace
runtime compatibility tests, and captured credentials/code must not become public
fixtures. Use disposable games; do not replace a listed game's active build to test.

## Source anchors

- Version admission: [`src/sdk_contract.rs`](../../src/sdk_contract.rs).
- Public imports: [`sdk/v1/infuser.js`](../../sdk/v1/infuser.js) and
  [`cloud.js`](../../sdk/v1/cloud.js); routes: [`sdk_v1.rs`](../../src/portal/sdk_v1.rs).
- Admission: [`sessions.js`](../../sdk/v1/sessions.js),
  [`hosting.mjs`](../../sdk/server/hosting.mjs), [dedicated contract](../dedicated-sessions.md).
- Existing evidence: [dedicated implementation results](dedicated-sdk-implementation.md),
  [roadmap](../roadmap.md), [`compat_client.mjs`](../../tests/compat_client.mjs),
  [frozen legacy fixture](../../tests/fixtures/legacy-sdk/README.md).
