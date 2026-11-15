# Existing PeerJS games

**Preview: tested with PeerJS 1.5.5 and PeerServer 1.0.2.** Keep the game's PeerJS
dependency, simulation, and message formats. Infuser supplies private connection
setup and admission; PeerJS still creates the real WebRTC DataConnections.
JSON and binary serialization are supported. Audio/video calls, mesh topologies,
multiple rooms per game, and seamless host migration are outside this adapter.

PeerJS is a library, not a new multiplayer modality. Declare `peer_hosted` and
`webrtc`, and set `adapter:"peerjs"` to select this adapter. Query publisher
capabilities for `sdk.capabilities` containing `peers.peerjs.v1` before editing or
publishing an integration. A host without the private PeerServer service rejects
the declaration; do not relabel the game as single-player to publish it.

```json
{
  "title": "The Existing Game Name",
  "play_modes": ["solo", "multiplayer"],
  "solo_runtime": "client_only",
  "sdk": {"major": 1, "api_major": 1, "required_capabilities": ["identity.v1", "peers.peerjs.v1"]},
  "multiplayer": {
    "mode": "peer_hosted", "transport": "webrtc", "adapter": "peerjs",
    "max_players": 4, "join_policy": "anytime",
    "direct_connect": "optional", "relay_fallback": "required"
  }
}
```

Keep both play modes only if the game actually supports both. Admission is one
host with up to seven members; the first participant hosts and everyone else
joins that session. A game with an existing host selection or room-code UI must
use Infuser's chosen host in its hosted path. Preserve its existing standalone
flow in the other path. This is connection setup work, not a promise of zero
changes to an arbitrary `net.js`.

## Keep one project working in both places

Copy [`sdk/optional.js`](../sdk/optional.js) into the project as
`infuser-platform.js`, then import it using a normal relative/bundled import.
Call `connectInfuser()` during startup. It returns `null` for standalone play and
makes no Infuser requests. The publisher injects the versioned SDK script into
the **uploaded export**; source HTML stays runnable outside Infuser. The HTTP
publishing recipe must insert that script too.

When the script is present, the helper loads the SDK and verifies the launch
through `ready`. Handle a rejected promise with retry/return-to-Library UI;
**never catch a failed Infuser login and silently use public matchmaking**.
Don't hardcode a particular Infuser hostname or infer authorization from a query
parameter. The script is only an environment marker; API authentication remains
server enforced. For standalone builds, keep `/_infuser/` imports out of static
module dependencies and don't copy the published HTML back into the source.

```js
import Peer from 'peerjs'; // Keep the project's existing import style/version.
import {connectInfuser} from './infuser-platform.js';

const infuser = await connectInfuser();
// Load the appropriate save provider here, before entering gameplay.

async function enterMultiplayer() {
  if (!infuser) return startExistingStandaloneMultiplayer();
  const room = await infuser.peerjs.join({
    Peer,
    joinable: false,
    connectionOptions: {serialization: 'binary', reliable: true},
    onConnection(connection, player) {
      // A real PeerJS DataConnection. Reuse the game's message handler.
      connection.on('open', () => sendCurrentGameState(connection));
      connection.on('data', message => handleGameMessage(player.id, message));
      connection.on('close', () => handleDisconnection(player.id));
    },
    onState(state) { /* state.role, state.peers, state.connected */ },
    onEnd(reason) { returnToTitleScreen(reason); }
  });
  if (room.role === 'host') {
    await initializeSharedGame();
    await room.setJoinable(true);
  }
  return room;
}
```

Use the game's existing serialization/reliability settings, not this example's
settings by default. Supported connection options are `serialization`, `reliable`,
`label`, and `metadata`. The adapter owns peer IDs, signaling URLs, ICE settings,
and connection lifecycle. Members initiate the connection to the host; both
sides receive `onConnection`. It may run again after recovery, so resend a current
snapshot and replace stale connection references. Do not create another `Peer`
or call public matchmaking in the hosted branch.

`connection.peer` and `room.id`/`room.hostId` are session-scoped PeerJS wire IDs.
The callback's `player.id` is the underlying coordinator peer ID;
`player.player_id` is the opaque account/guest identity used by presence.
Use the latter for identity, not a PeerJS connection ID. `room.peers` retains the
coordinator IDs and adds `peerjs_id` for matching DataConnections. Each connection accepts
the game's original message types. The adapter fences stale incoming data and
returns `false` from `send()` if closed, expired, or congested; a successful send
is not an application-level acknowledgement. Validate messages in the game.

`room.active`, `room.epoch`, `room.role`, `room.peers`, and `room.connections`
provide current state. `room.setJoinable(boolean)` is host-only. `room.close()`
leaves multiplayer without ending the single-player launch. `room.stats()` is for
adult diagnostics/testing; the existing title-bar symbol stays quiet and does
not disturb fullscreen.

## Saves and Join friend

Choose the save provider at startup: Infuser cloud saves for an authenticated
hosted launch, the game's existing local saves otherwise. Guest launches use the
separate Infuser namespace. Do not automatically import standalone/local progress
into a cloud or guest namespace. Follow the [cloud save contract](cloud-save-design.md).

For [Join friend](launch-intents.md), declare `launch_intents:["join_friend"]`,
consume the intent once, and pass its `session_id` as `sessionId` and `player_id`
as `playerId` to `infuser.peerjs.join`. A stale/full/closed target fails visibly;
don't retry without the target and accidentally create another world. Use
`infuser.presence.get()` for the title screen; it does not join a room.

## Recovery and acceptance

Normal play permits direct WebRTC paths and retains mandatory TURN fallback.
`relayOnly:true` forces TURN for acceptance; the manifest can also prohibit direct
connections. The adapter renews short-lived TURN credentials, updates ICE config,
and recreates member data connections. Temporary signaling failures retry within
the coordinator lease. Expired leases, host departure, and account revocation
end the session; a resumed laptop cannot revive an old host. Rejoin explicitly.

Before calling a migrated game done, verify:

- Hosted and standalone play using the same source project; standalone startup
  makes no Infuser requests and keeps its original matchmaking/saves.
- Host/member messages in the game's actual format, including any binary data.
- Forced relay with selected relay/relay candidates and byte counters increasing
  in both directions; candidate gathering alone is insufficient.
- Closed connection recovery, transient signaling failure, TURN renewal, and
  leaving/sleeping past the lease without a second host appearing.
- Guest isolation, Join friend, and the game's own save/UI behavior.

The platform fixture covers networking; it does not certify an unseen game's
simulation or save integration. File issues using the [agent reporting instructions](agents.md#reporting-platform-problems)
with a sanitized repro and PeerJS version.

## Operator setup

One shared private PeerServer stays running for the installation; it does not
start per game. Games declare the adapter, and rooms form when they enter
multiplayer. PeerServer handles signaling only. The Rust gateway verifies
the game launch, origin, session epoch, host/member edge, and current membership
on signaling messages, and checks idle sockets regularly. TURN carries relayed
game traffic. Neither Google cookies nor player tickets are forwarded upstream.
Public PeerServer discovery and its ID-generation endpoint are not exposed.

With TURN already installed, build the pinned sidecar and bind it **only to
loopback**. Use a checkout matching the daemon release:

```sh
docker build -t infuser-peerjs services/peerjs
# Pin the resulting local image ID, not a mutable tag, for the running container.
image=$(docker image inspect --format '{{.Id}}' infuser-peerjs)
docker run -d --name infuser-peerjs --restart unless-stopped \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  --memory 128m --cpus 0.25 --pids-limit 64 \
  --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  --log-opt max-size=5m --log-opt max-file=2 \
  -p 127.0.0.1:7780:9000 "$image"
```

Add `peerjs_port = 7780` inside `[controller]` in the private server config, then
restart the daemon. Requires a daemon release with PeerJS support. The existing
Caddy routing covers `/_infuser/peerjs/peerjs`; no new public ports or DNS records
are needed. Leave the PeerServer port outside game/TURN allocations. A missing
sidecar fails closed; there is no public-server fallback. Existing Infuser and
Godot peer games keep their original adapters and require no changes.

The regression suite checks gateway authorization and fast adapter recovery.
For a full local browser run, `npm ci --prefix services/peerjs`, install its
Playwright Chromium if needed, build `cargo build --example peerjs_fixture`, then
run `node tests/peerjs_journey.mjs`. The fixture binds only loopback and uses fake
accounts; it is never compiled into the production daemon. A private file with
two preview launch URLs can instead drive `node tests/peerjs_journey.mjs FILE recovery`
against real TURN. Never commit or print those launch URLs.
