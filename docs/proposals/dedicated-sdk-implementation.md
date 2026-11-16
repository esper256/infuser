# Dedicated SDK implementation plan

Audience: Infuser developers.

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
4. **Lifecycle.** Keep website logout independent of ongoing play; define renewal, guest
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

## Implementation and verification

The candidate now includes the verified gateway, scoped process registration,
sequenced full room reports, admission results, browser and Node helpers, dedicated
presence, Library/avatar Join Friend targets, reconnect reservations and draining.
Both new capabilities are opt-in. Existing transports and manifests keep working.
See [the game integration contract](../dedicated-sessions.md) for actual names and
payloads; earlier proposal examples are not the wire contract.

Verified locally:

- Real sockets with two members and a guest, unchanged text/binary frames, forged
  identity and replay rejection, cancellation, lease release and readable failures.
- Ordered snapshots, duplicate/lost-response retries, retired process credentials,
  expired tokens and private-group scope.
- Real Chromium, Node ws and the production Rust gateway: cold start, first welcome
  delivery, Join Friend intent, distinct guest identity, multiple runs, exact resume,
  simultaneous last-seat requests and no new world after a failed targeted join.
- Queued matched updates, idle title screens allowing activation, old-client refusal,
  shared policy, a non-root WebSocket path and negotiated subprotocol.
- Website logout intentionally leaves existing play connected. Disabled membership
  still ends access; games do not implement platform revocation themselves.
- Frozen renderer/background operation and full server credential renewal are
  exercised by the optional longer browser journey, outside fast default checks.

The live ARM64 Docker/Caddy journey also passed: source packaging, two distinct
guest identities, exact Join Friend, a separate single-player room, publisher
diagnostics, controller restart with both players resuming the same process/run,
draining updates, old-client refusal, a declared path/subprotocol and idle shutdown.
The hidden fixture was removed after testing; family games were not republished.

Lanternfall itself remains untouched. Its agent can now integrate against the
documented contract. Keep SDK 1.0 a candidate until that real-game integration
confirms the contract. Browser-only single player can coexist with managed
multiplayer; the platform does not require every mode to use the server.
