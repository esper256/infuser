# In-game presence and joinability

Audience: game authors and their agents.

**Experimental v1; the overall SDK is still pre-1.0.** Capabilities advertise
`presence.version = 1` and `presence.joinability` includes `peer_hosted_webrtc`
and `dedicated_websocket`. Presence is readable by any authenticated game launch.
Managed server occupancy alone does not mean that a game is joinable: opt into
[dedicated sessions](dedicated-sessions.md) and report actual rooms and admissions.

Dedicated games receive an additive `sessions` array containing
`{id,label,activity,joinable,max_players,players}`; each player has
`{player_id,name,guest}`. Use `peer_sessions` for peer-hosted rooms.
Use `sessions.connect({action:'join',target:{session_id,player_id}})` for a dedicated
target. Reports expire after 45 seconds; new joins are unavailable while an update
drains existing runs. This adds `server_draining` to `unavailable_reason`.
The rest of this page describes peer rooms.

## What a game learns

```js
import { ready, presence, peers } from '/_infuser/sdk/v1.js';
const me = await ready;
if (me.capabilities.presence?.version === 1) {
  const snapshot = await presence.get();
  // Render names using textContent, never HTML. Poll about every five seconds.
  // Keep single player usable if presence is unavailable.
}
```

`GET /_infuser/api/v1/presence/v1` returns:

```json
{
  "app_id": "pond",
  "build_id": "one",
  "self_id": "opaque-player-id",
  "players": [
    {"player_id":"opaque-player-id","name":"River","guest":false,"is_self":true,
     "online":true,"game_open":true,"multiplayer":false},
    {"player_id":"guest-opaque-launch-id","name":"Guest","guest":true,"is_self":false,
     "online":true,"game_open":true,"multiplayer":true}
  ],
  "peer_sessions": [{
    "id": "opaque-session-epoch",
    "build_id": "one",
    "matchmaking": false,
    "host_player_id": "guest-opaque-launch-id",
    "player_count": 1,
    "max_players": 4,
    "joinable": true,
    "reason": null,
    "players": [{"player_id":"guest-opaque-launch-id","name":"Guest","guest":true}]
  }],
  "can_create": true,
  "unavailable_reason": null
}
```

- `online` means recent Library/game activity by an enabled invited member, or a
  current guest in this game. It does not mean playing multiplayer.
- `game_open` means a recently heartbeating launch of this game, including title
  screens and single player. No claim about gameplay mode is inferred.
- `multiplayer` means actual membership in this game's live peer session.
- Accounts are deduplicated by opaque player ID. Guests have their own identity
  and the name Guest; they do not pretend to be the signed-in owner. Multiple
  independent guest launches can appear with the same display name.

Online/game-open activity ages out after 90 seconds without heartbeat activity.
Peer membership uses its existing 30-second coordinator lease. Disabled members
and expired/unexchanged launches do not appear. A snapshot prunes invalid peer
launches; if that removes the host, the session ends. It contains no emails,
tickets, credentials, signaling payloads, telemetry, or other games' metadata.
The Library lists players within the current private group or visit. The web
player reports a departing tab so the Library can remove the game on its next
refresh after the last tab closes. Switching tabs keeps the game listed. When a
browser exits without sending that report, the 90-second heartbeat timeout applies.
Game authors do not need to add a close handler.

Use `peer_sessions` for the room picker. Show each host's name by matching
`host_player_id` to a room member. `can_create` indicates whether this build
can start a fresh room, even when others are full. Reading presence reserves
nothing. The room marked `matchmaking: true` also appears in the legacy
`session` field. That field is null when there is no automatic matchmaking room.
For peer games, legacy `can_start` means a new matchmaking room can start.
`unavailable_reason` is null or `unsupported_topology`, `relay_unavailable`,
`newer_build`, or `coordinator_restarting`. After a controller restart, new peer sessions wait for old connections to expire.

Session `reason` is null when joinable, otherwise `different_build`, `full`,
`closed`, or `not_advertised`. Occupancy counts peer connections, matching admission
capacity. Multiple connections for one account can consume multiple slots even
though the presence player list deduplicates that account.

The examples below use the browser peer helper. For PeerJS or Godot, use the
matching [adapter](agents.md#choose-the-integration) with the same presence
snapshot and session target. Do not replace an existing adapter just for presence.

## What a host reports

```js
const room = await peers.create({joinable:false});
// Initialize the host simulation, then:
if (room.role === 'host') await room.setJoinable(true);
// During a round that cannot accept newcomers:
if (room.role === 'host') await room.setJoinable(false);
```

For peer admission the host reports a boolean: whether this session accepts new participants.
Names, identities, occupancy and capacity come from authenticated launches and
the manifest. Only the verified host connection can update admission, with its
matching session ID (`epoch` in the peer API). Members cannot update it; IDs from
ended sessions are rejected.
Closing admission leaves everyone already in the room connected, and an existing
connection can retry its join without being rejected. Full rooms stay unjoinable
even if the host's `acceptingPlayers` preference is true. A departing member frees
a slot; host departure ends the session as before.

The HTTP control is `PUT /_infuser/api/v1/peers/PEER/joinability` with
`{"epoch":"CURRENT_SESSION_ID","joinable":true}`. It requires the same exact
Origin and launch authentication as other peer mutations. It returns the peer
view, including `accepting_players` and `max_players`. Admission updates do not
renew the host lease; the normal signaling poll does that.

## Joining what the player saw

For a displayed session, capture its ID when the player chooses Join:

```js
const observed = snapshot.peer_sessions.find(room => room.id === selectedRoomId);
if (observed?.joinable) {
  try {
    const room = await peers.join({sessionId:observed.id});
    // Start this game's multiplayer flow using the returned room.
  } catch (error) {
    // Stay on/return to the title screen, show why joining failed, refresh presence.
  }
}
```

The wire field is `expected_epoch` on `POST /_infuser/api/v1/peers`. If the room ended or
was replaced, return 410 without creating a room or appointing a host. Closed/full
rooms return 409. A stale build also returns 409 and must reopen from the Library.
Do not silently retry without the session ID: that would change the player's
intent. [Launcher Join friend intents](launch-intents.md) use this same guard,
plus a target player ID so a friend leaving also causes a visible failure.

Use `peers.create()` for Host or the default co-op Play button. It always hosts
a fresh room and opens admission by default. The HTTP equivalent is
`POST /_infuser/api/v1/peers` with `{"create":true,"connection_id":"<uuid>"}`.
Retry the same live connection ID to receive the same room. For Join Friend pass
both `sessionId` and `playerId` to `peers.join`. Older untargeted `peers.join()`
calls still use a shared matchmaking room and cannot take a seat in a room
someone explicitly created.

Old clients that omit `joinable` preserve their existing ordinary-join behavior.
Their rooms have `accepting_players:null` and are not advertised as joinable.
New games opt in by passing a boolean on creation or calling `setJoinable` as host.
The creation option is ignored for member joins and repeated host joins; use the
explicit host method to change it. Old games still work without presence calls.

Presence reads do not renew browser or peer leases, start servers, reserve seats,
or alter fullscreen. The normal SDK heartbeat remains required. Cache only briefly,
handle fetch failures, and show a small player list or Join action in the game's own
title screen. Numeric network diagnostics remain in admin tools.

The Library shows **Online** for two minutes after input, **Idle** until ten
minutes, then **Away** while the player remains connected. Keyboard, pointer,
wheel and gamepad input in the hosted web player count. Library polls and game
heartbeats establish reachability only. A disconnected player drops from the
online strip when their 90-second presence expires. The optional
[game mode report](launch-intents.md#implement-join-friend-from-the-library)
describes what the player is doing separately from these indicators.
