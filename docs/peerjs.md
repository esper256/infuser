# Existing PeerJS games

Audience: game authors and their agents.

**Preview: tested with PeerJS 1.5.5 and PeerServer 1.0.2.** Keep the game's PeerJS
dependency, simulation, and message formats. Infuser supplies private connection
setup and admission; PeerJS still creates the real WebRTC DataConnections.
JSON and binary serialization are supported. Audio/video calls, mesh topologies,
multiple rooms per game, and seamless host migration are outside this adapter.

PeerJS is a library, not a separate multiplayer mode. Declare `peer_hosted` and
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

Keep both play modes only if the game actually supports both. Each session has one
host with up to seven other players; the first participant hosts and everyone else
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
the game's original message types. The adapter ignores data from expired connections and
returns `false` from `send()` if closed, expired, or congested; a successful send
is not an application-level acknowledgement. Validate messages in the game.

`room.active`, `room.epoch`, `room.role`, `room.peers`, and `room.connections`
provide current state. `room.setJoinable(boolean)` is host-only. `room.close()`
leaves multiplayer without ending the single-player launch. `room.stats()` is for
admin diagnostics/testing; the existing title-bar symbol stays quiet and does
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

## Reconnects and testing your game

Normal play permits direct WebRTC paths and retains mandatory TURN fallback.
`relayOnly:true` forces TURN for testing; the manifest can also prohibit direct
connections. The adapter renews short-lived TURN credentials, updates ICE config,
and recreates member data connections. Temporary signaling failures retry within
the coordinator lease. Expired leases, host departure, and account revocation
end the session; a resumed laptop cannot revive an old host. Rejoin explicitly.

Follow the [publication checks](agents.md#publish-and-verify) first. If an
authorized game test session is available, also check the following. Otherwise,
mark live checks as untested and hand the published game to its player. Do not
request Google account access to run these checks.


- Hosted and standalone play using the same source project; standalone startup
  makes no Infuser requests and keeps its original matchmaking/saves.
- Host/member messages in the game's actual format, including any binary data.
- Forced relay with selected relay/relay candidates and byte counters increasing
  in both directions; candidate gathering alone is insufficient.
- Closed connection recovery, transient signaling failure, TURN renewal, and
  leaving/sleeping past the lease without a second host appearing.
- Guest isolation, Join friend, and the game's own save/UI behavior.

File issues using the [agent reporting instructions](agents.md#reporting-platform-problems)
with a sanitized repro and PeerJS version.
