# Peer-hosted web games (experimental)

This browser JSON-message adapter is tested with Three.js/Vite. The game owns
its title screen and
chooses when to join multiplayer. Single player stays local. There is one shared
session per app, with at most eight browser peers; the first participant hosts.
This transport is pre-1.0 and needs the same contract review as the save SDK.

Existing PeerJS games can use the [PeerJS adapter](peerjs.md) instead of replacing
their DataConnection message handling with this JSON helper. Both share the same
session, presence, Join friend, and mandatory relay rules. Declare the adapter
explicitly; their signaling protocols and message framing are not interchangeable.

Declare both supported play modes in `infuser.game.json`:

```json
{
  "sdk": {
    "major": 1,
    "api_major": 1,
    "required_capabilities": ["peers.webrtc.v1"]
  },
  "play_modes": ["solo", "multiplayer"],
  "solo_runtime": "client_only",
  "multiplayer": {
    "mode": "peer_hosted",
    "transport": "webrtc",
    "max_players": 4,
    "join_policy": "anytime",
    "direct_connect": "optional",
    "relay_fallback": "required"
  }
}
```

No game Docker image is needed. Publish with the normal CLI, without `--solo`.
Publishing capabilities report whether the installation has configured TURN.
`direct_connect: optional` permits optimization; TURN support remains mandatory.
Games explicitly control admission with `room.setJoinable(boolean)`. For
`between_rounds`, close admission during the round and reopen between rounds.
Closing admission does not remove existing players. See [presence and
joinability](presence.md) for title-screen discovery and safe targeted joins.

```js
import { peers } from '/_infuser/sdk/v1.js';
const room = await peers.join({
  relayOnly: false, // Normal play: allow direct paths, retain TURN fallback.
  joinable: false, // Keep new players out until the host's game state is ready.
  onState(state) { /* state.peers, state.connected, state.role */ },
  onMessage(from, data) { /* validate game messages before applying */ },
  onEnd(reason) { /* stop simulation and offer a return to the title screen */ }
});
if (room.role === 'host') await room.setJoinable(true);
// Inside the game loop, after validating messages:
if (room.role === 'host') room.broadcast({ type: 'snapshot', tick: 1 });
else room.send(room.hostId, { type: 'input', x: 0, z: 1 });

// When the player leaves multiplayer:
room.close();
```

Normal peer-hosted play prefers LAN host candidates, then direct public-address
paths, then TURN. ICE prioritizes and checks candidate pairs; it does not impose
three long sequential timeouts, and the selected working path depends on the
browser and network. UPnP is not required. SDK major 1 defaults to allowing direct
paths; the legacy SDK retains its old default. `direct_connect: disabled` in the
published manifest still enforces relay-only behavior, regardless of the client
option. Use `relayOnly: true` for a forced-fallback acceptance run, and prove the
selected pair is relay/relay with traffic in both directions.

The [Godot web study](engines/godot-web.md) uses the same coordinator and relay,
but Godot owns its `WebRTCMultiplayerPeer` channels and RPCs. The JSON messaging
API below describes the browser helper, not Godot's wire protocol.

To join the session shown on a title screen, pass `sessionId: snapshot.session.id`
to `peers.join()`. This must fail if that session ended or stopped accepting
players; it never appoints a replacement host. A new session uses `joinable` only
on creation. A member or a repeated join cannot change the host's admission policy.
Existing integrations omitting `joinable` retain ordinary join behavior, but their
rooms are not advertised as joinable until the host explicitly sets it.

Messages are JSON, limited to 64 KiB, over an ordered reliable DataChannel.
`send` returns false when disconnected or congested: games should keep their
latest state and try again, rather than queue every missed frame. The SDK does
not provide reliable application-level acknowledgments, rollback, anti-cheat,
simulation, save ownership, or host migration. The game must validate inputs,
check player/host roles, and tolerate reconnects. Avoid sending secrets to peers.

Brief signaling failures retain the current session while its coordinator lease
is valid. The host rebuilds failed peer links inside that same session; games
should send their latest authoritative snapshot when a member reconnects. Once
the lease expires, `close()` is called, or the host departs, the room is terminal:
late negotiation results and obsolete channel events cannot revive it. `onEnd`
fires once. Rejoining requires an explicit new `peers.join()` call. A channel
closing during `send()` returns false rather than throwing into the game loop.

The controller handles authenticated HTTP signaling only; game traffic flows
through WebRTC. Host/member edges form a star. Each connection is bound to a
verified game launch and an opaque session epoch. HTTP signaling queues are
bounded, replay until acknowledged, and reject unrelated launches. TURN
credentials expire after an hour; the SDK refreshes them and rebuilds host
connections every 20 minutes. Guest players receive separate identities.

The SDK stops sending and accepting packets when its 25-second coordinator
lease lapses. The server removes an unresponsive host at 30 seconds, ending the
session for everyone. A controller restart waits 30 seconds before appointing a
new host, fencing old browser sessions. Closing the host ends the session;
refresh/rejoin starts a new one. No seamless host migration is promised. Browser
background throttling can slow a browser-hosted simulation; games should surface
host connection status and allow leaving/rejoining.

A publish during an active peer session queues the new build. When the session
ends, the controller promotes it; already-open old clients must reopen before
starting another multiplayer session. Single-player saves are unchanged.

## Installing the shared relay

Run coturn alongside the controller, with the same private shared-secret file.
See [the configuration template](../deploy/turnserver.example.conf). Use an
ARM64-compatible image where appropriate, pinned to a tested digest. With the
current official coturn image, retain `NET_BIND_SERVICE` when dropping other
capabilities: the executable has a file capability. Run as a non-root UID, with
read-only storage, a small writable `/tmp`, CPU/RAM/PID limits, host networking,
and log rotation. The relay does not need the Docker socket.

Reserve non-overlapping listener and relay ports outside the game allocator.
For example, reduce controller `port_end` to 8699 and add:

```toml
[controller.turn]
urls = ["turn:games.example.com:8700?transport=udp", "turn:games.example.com:8700?transport=tcp"]
secret_file = "/etc/infuser/turn.secret"
relay_port_start = 8800
relay_port_end = 8899
```

Generate a random shared secret locally on the server. Protect it and the coturn
config from other users; never commit either. Allow the listener (UDP/TCP) and
relay range (UDP) in both the cloud and host firewall. Keep the installation's
login-based IP admission rules covering these ports. Set coturn `external-ip`
when behind a public/private IP mapping. Check the provider's NAT hairpin behavior
when both peers use this same relay: allocations must reach each other's public
relay addresses. Any necessary hairpin rule should match only this server's IP
and the reserved relay ports, not all outbound traffic.

The example uses TURN UDP/TCP, with WebRTC DTLS protecting game payloads; it does
not configure TURN-over-TLS. Networks requiring TLS-only traversal are a future
compatibility target. Setting `relayOnly: true` and seeing **relay ↔ relay** in
selected-candidate stats with increasing byte counters is the acceptance test;
merely gathering a relay candidate does not prove gameplay used it.

## Connection status and adult diagnostics

Players see only a small connection symbol in the existing title bar. It does
not overlay the game, open dialogs, or change fullscreen. A green connection
means the peer links are connected; it does not claim that device rendering or
the game's simulation is fast. Games should not add numeric network overlays to
the normal player experience.

The SDK sends bounded browser connection reports every five seconds, stored only
in memory for the active peer session. Adult accounts can inspect them under
Admin tools → Network diagnostics. `GET /api/admin/peer-health` requires adult
Google membership; the equivalent loopback-only `GET /v1/peer-health` uses the
existing operator bearer credential. Reports include selected candidate types,
byte counters, and the browser's current ICE round-trip estimate when available.
These are diagnostic observations, not proof of device performance or blame for
lag. Reporting failures never stop gameplay. The diagnostics contract remains
experimental and is separate from game state and cloud saves.

## Fast recovery checks

`node --test tests/*_sdk.mjs` runs the shipped SDK against controllable signaling
and WebRTC boundaries, with a simulated clock. It covers transient signaling
failure, link replacement, tab sleep/lease expiry, leaving during host/member
negotiation, stale events, and send/close races without real-time sleeps. These
checks complement the HTTP peer journeys and live two-browser TURN dogfood;
they do not claim to simulate a browser's actual ICE implementation.
