# Dedicated SDK implementation plan

Accepted direction following [issue #33](https://github.com/esper256/infuser/issues/33).
SDK 1.0 remains a candidate. Existing games keep their current routes and behavior.

## Decisions that must survive 1.0

- Infuser owns authenticated connection lifetime. Games receive verified context
  and ordinary socket events; they do not implement platform revocation or need
  to know which proxy enforces it. Keep gameplay frames unchanged.
- A launch, process instance, connection attempt, and gameplay session are different
  identities. Scope all authority to installation, private group, game, build,
  instance, and launch. An attempt handle is not a bearer credential.
- Games own room creation and atomic seat admission. An exact-session join succeeds
  in that incarnation or fails; it never creates a replacement run. A connected
  transport alone is not proof of room admission.
- Preserve typed attempt results outside WebSocket close events. Retries recover
  the same outcome; cancelled or consumed attempts cannot open another socket.
- Discovery does not grant access or renew demand. A server report alone cannot
  keep a container alive. Valid live connections support backgrounded players;
  disconnects get bounded recovery grace, not infinite renewal.
- Keep hosted and standalone paths in one project. Missing platform configuration
  may select standalone; invalid or incomplete hosted configuration must fail closed.

## Build order and completion evidence

1. **Verified connection boundary.** Add instance-bound signed identity proofs,
   a small server verifier, and a Rust WebSocket gateway with bounded buffers,
   handshake/write deadlines, ping liveness and platform-enforced revocation.
   Exercise real sockets for two members and a guest, forged identity, replay,
   other-origin access, member disable, release and stale process identity.
   Do not advertise a game-facing capability before its adapter and contract pass.
2. **Admission and discovery.** Add scoped process credentials, expiring sequenced
   session snapshots and admission results. Separate per-room capacity from process
   capacity. Support shared and explicitly multiple sessions. Test full-room races,
   duplicate/lost responses, retired instances and cross-group rejection.
3. **Join Friend.** Extend presence additively and route Library, avatar and in-game
   targets through the same admission path. Add a browser helper returning an ordinary
   WebSocket only after admission. Preserve current peer APIs and title-screen choice.
4. **Lifecycle.** Bind logout to its browser login session; define renewal, guest
   expiry, background operation, reconnect reservations and bounded outage grace.
   Add drain notices and prevent new runs while an old client/server pair finishes.
   Test controller restart, vanished clients, delayed release and queued activation.
5. **Integration and candidate freeze.** Exercise the same contract in the dedicated
   fixture and a different room-admission flow. Supply a small Node helper and local
   development harness using the production protocol. Test real Docker/Caddy before
   advertising support. Then let Lanternfall's agent adapt its game; this project
   does not edit Lanternfall. Freeze only capabilities that pass these journeys.

Each step should leave a tested, reviewable change. Keep incomplete entry points
unadvertised and clearly identified as internal scaffolding; no game should need
to integrate against half of a contract. Record remaining gaps rather than calling
hosting success a completed social integration. No extra CI or approval process.

## Current implementation checkpoint

The connection foundation is implemented and tested locally, **not advertised or
available to published games yet**. Publishing rejects `connections.websocket.v1`
until the rest of its integration is ready. Existing SDKs and server routes retain
their behavior.

- The Rust gateway supplies an Ed25519 proof with installation, group, game, build,
  instance, launch, connection and player identity. Game containers receive only
  the public key. Caller identity headers, cookies and bearer credentials are not
  forwarded. The signing key survives controller restart in a private file.
- The internal Node verifier rejects forged, expired, replayed and wrongly scoped
  proofs. Missing all hosted configuration selects standalone; partial hosted
  configuration fails. Instantiate one verifier per server process.
- Authenticated attempt preparation is idempotent. An epoch prevents retries from
  crossing controller restarts; terminal handles remain consumed for the launch's
  lifetime. Status is readable separately from the WebSocket handshake. `connected`
  means transport established, **not room admission**.
- The gateway checks current membership, group, build, process and lease while
  forwarding unchanged text/binary messages. Release, cancellation and membership
  disable close connections without a game-side handler. Transport pongs can renew
  the existing lease without relying on browser JavaScript timers.
- Buffers, pending attempts, handshakes and writes are bounded. The current internal
  defaults are 1 MiB messages, four live attempts per launch, 256 installation-wide,
  and 4096 retained attempt records. These are implementation bounds, not promised
  SDK capacities; effective limits need an advertised contract before adoption.

The real-socket journey covers two members plus a guest, text/binary forwarding,
identity forgery, duplicate upgrade, foreign origin, attempt ownership, cancellation,
membership revocation, release and a failed upstream handshake with readable status.
Key persistence and replay-tombstone behavior have focused tests. Existing runtime
and PeerJS journeys remain unchanged.

Still outstanding: atomic room admission and its typed errors; per-instance server
credentials; session reports; the browser and complete `ws` adapters; Join Friend;
logout binding; measured background/reconnect behavior; drain control; subprotocol
and endpoint-path requirements; real Docker/Caddy validation. The current socket
liveness mechanism does not establish completion of those lifecycle acceptance items.
