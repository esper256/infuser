# In-game presence and joinability

**Experimental v1; the overall SDK is still pre-1.0.** Capabilities advertise
`presence.version = 1` and `presence.joinability` includes `peer_hosted_webrtc`
and `dedicated_websocket`. Presence is readable by any authenticated game launch.
Managed server occupancy alone does not mean that a game is joinable: opt into
[dedicated sessions](dedicated-sessions.md) and report actual rooms and admissions.

Dedicated games receive an additive `sessions` array containing
`{id,label,activity,joinable,max_players,players}`; each player has
`{player_id,name,guest}`. The singular `session` below remains the peer-room field.
Use `sessions.connect({action:'join',target:{session_id,player_id}})` for a dedicated
target. Reports expire after 45 seconds; new joins are unavailable while an update
drains existing runs. This adds `server_draining` to `unavailable_reason`.
The rest of this page describes the unchanged peer API.

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
  "session": {
    "id": "opaque-session-epoch",
    "build_id": "one",
    "host_player_id": "guest-opaque-launch-id",
    "player_count": 1,
    "max_players": 4,
    "joinable": true,
    "reason": null,
    "players": [{"player_id":"guest-opaque-launch-id","name":"Guest","guest":true}]
  },
  "can_start": false,
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
All invited members are the current social circle; no separate friend list exists.

`session` is null when no live room exists. `can_start` is true only when a room
can be created for this published build and topology; reading it reserves nothing.
Its availability is a hint, not a promise that the later join cannot fail.
`unavailable_reason` is null or `unsupported_topology`, `relay_unavailable`,
`newer_build`, or `coordinator_restarting`. The startup restart fence still applies.

Session `reason` is null when joinable, otherwise `different_build`, `full`,
`closed`, or `not_advertised`. Occupancy counts peer connections, matching admission
capacity. Multiple connections for one account can consume multiple slots even
though the presence player list deduplicates that account.

The examples below use the browser peer helper. For PeerJS or Godot, use the
matching [adapter](agents.md#choose-the-integration) with the same presence
snapshot and session target. Do not replace an existing adapter just for presence.

## What a host reports

```js
const room = await peers.join({joinable:false});
// Initialize the host simulation, then:
if (room.role === 'host') await room.setJoinable(true);
// During a round that cannot accept newcomers:
if (room.role === 'host') await room.setJoinable(false);
```

The only report is a boolean: whether this session accepts new participants.
Names, identities, occupancy and capacity come from authenticated launches and
the manifest. Only the verified host connection can update admission, with its
matching session epoch. Members cannot update it; stale epochs are rejected.
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
const observed = snapshot.session;
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

An ordinary Multiplayer choice with no observed session may call `peers.join()`
without `sessionId`: it starts the one shared room, or joins a concurrently created
room if that room permits admission. It never creates a parallel world.

Old clients that omit `joinable` preserve their existing ordinary-join behavior.
Their rooms have `accepting_players:null` and are not advertised as joinable.
New games opt in by passing a boolean on creation or calling `setJoinable` as host.
The creation option is ignored for member joins and repeated host joins; use the
explicit host method to change it. Old games still work without presence calls.

Presence reads do not renew browser or peer leases, start servers, reserve seats,
or alter fullscreen. The normal SDK heartbeat remains required. Cache only briefly,
handle fetch failures, and render a small social affordance in the game's own
title screen. Numeric network diagnostics remain in adult tools.
