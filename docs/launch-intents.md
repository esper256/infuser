# Join friend launch intents

Audience: game authors and their agents.

**Experimental v1, for peer-hosted WebRTC and managed dedicated WebSocket games.** Ordinary Play opens the
game's title screen. Join friend is an explicit request to join a specific person
in a specific existing session. It never means creating a replacement world.

## Entry points

The Library offers Join Friend in the game-card dropdown and beside online-player
avatars. Both send the same request to the game.

`GET /api/library` adds `apps[].join_targets` for visible, opted-in games with an
explicitly joinable room. Each target has `session_id`, `player_id`, `name`, and
`guest`. The viewer's own account is excluded. Full/closed rooms have no targets.
`friends[].join_targets` carries `app_id`, `title`, `session_id`, and `player_id`
for avatar entry points. Hidden previews are filtered before building
either list. Presence elsewhere never makes a hidden game discoverable.

When a target is selected, send:

```json
{
  "guest": false,
  "intent": {
    "kind": "join_friend",
    "session_id": "OBSERVED_SESSION_ID",
    "player_id": "OBSERVED_PLAYER_ID"
  }
}
```

to `POST /api/apps/APP/launch` using the normal Google membership/Origin checks.
Do not combine an intent with legacy `mode`. Guest launches can carry an intent
through the API, with the same isolated identity/save rules as other guest play.
The card menu's Join action uses the signed-in player's progress.

## Game opt-in and consumption

Add `"launch_intents":["join_friend"]` to `infuser.game.json`; the publisher checks
`capabilities.launch_intents.version === 1` before upload. The manifest is accepted
for peer-hosted WebRTC or builds declaring [dedicated sessions](dedicated-sessions.md).
An unsupported game's launch rejects an intent
with 422 rather than pretending it can handle it. Old builds need no changes.

Dedicated games pass the consumed intent to `Infuser.sessions.connect({intent})`.
The helper checks that the exact session and friend are still present. It never
creates a replacement run. Ordinary Play still opens the game's title screen.

For peer games, after loading its state, consume the optional intent once:

```js
import { ready, launchIntent, peers } from '/_infuser/sdk/v1.js';
const context = await ready;
if (context.capabilities.launch_intents?.version === 1) {
  try {
    const { intent } = await launchIntent.consume();
    if (intent?.kind === 'join_friend') {
      const room = await peers.join({
        sessionId: intent.session_id,
        playerId: intent.player_id,
        relayOnly: false
      });
      // Enter this game's multiplayer flow using room.
    }
  } catch (error) {
    // Show the title screen and a short explanation. Never retry as a new host.
  }
}
```

The wire operation is `POST /_infuser/api/v1/launch-intent/v1/consume`, returning
`{"intent":null}` for ordinary Play or a previously consumed intent. It requires
the exact game Origin and scoped launch cookie/ticket. Consumption is atomic and
durable: simultaneous calls deliver to at most one caller. `GET context` never
consumes it, so the enclosing player frame cannot steal the game's intent.

This is best-effort, **at-most-once** delivery. If the consume response is lost,
show the title screen; automatic retries must not cause a surprise multiplayer
transition. The SDK does not retry it. Refreshing after consumption does not replay
the request. If a game cannot act because of unresolved saves, consume/discard
the intent and explain why; don't leave it waiting to fire on a later refresh.

Join launches receive a separate browser origin, even for signed-in players. This
prevents a new launch's cookie/intent from replacing an already-open game's cookie.
Personal save ownership remains player/app-scoped across both origins.

## Staleness and compatibility

Library information can go stale. A valid intent can still launch the game after
the room closes or the friend leaves, allowing its title-screen fallback. Actual
joining rechecks the session ID, target membership, build, capacity and host's
joinability together. The peer wire fields are `expected_epoch` and
`expected_player`. Here, `epoch` means the observed session ID. A friend target requires it.

For peer-hosted games, room replacement/ending or the friend leaving returns 410; closed/full rooms or
build mismatch return 409. Do not drop either target field and retry. A failed
join leaves the current shared room unchanged. All existing membership and relay
requirements still apply.

Dedicated sessions use typed `error.code` values such as `session_ended`,
`friend_left` and `session_full`, normally with HTTP 409. Their Node helper and
browser SDK handle admission together; do not use peer signaling fields or
require TURN for this transport. See [dedicated sessions](dedicated-sessions.md).
