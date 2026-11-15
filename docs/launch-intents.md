# Join friend launch intents

**Experimental v1, currently peer-hosted WebRTC only.** Ordinary Play opens the
game's title screen. Join friend is an explicit request to join a specific person
in a specific existing session. It never means creating a replacement world.

## Entry points

Keep the primary Play button and hero artwork clear. Join actions belong
in the game card's arrow dropdown alongside guest play. The same action can be
used beside online-player avatars near the logo; the avatar strip's visual design
is separate from this API. Do not add permanent Join buttons over game art.

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
only for peer-hosted WebRTC games. An unsupported game's launch rejects an intent
with 422 rather than pretending it can handle it. Old builds need no changes.

After loading its state, the game consumes the optional intent once:

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
admission rechecks the session epoch, target membership, build, capacity and host's
joinability atomically. The peer wire fields are `expected_epoch` and
`expected_player`. A friend target requires an epoch.

Room replacement/ending or the friend leaving returns 410; closed/full rooms or
build mismatch return 409. Do not drop either target field and retry. A failed
join leaves the current shared room unchanged. All existing membership and relay
requirements still apply.

Manifest opt-in is stored beside the legacy catalog so older daemons can read and
rewrite that catalog without losing the declaration. Pending intents are short-
lived launch metadata: restart on the same version preserves them, while rollback
to an older binary may discard them. Falling back to a title screen is intentional;
there is no promise to replay an intent across a downgrade. Saves are unaffected.

For disposable hidden previews, the operator helper supports:
`infuser preview-game CONFIG APP ADMIN_EMAIL OUTPUT --join-session SESSION --join-player PLAYER`.
It retains the existing enabled-adult/hidden-game checks and writes a private,
single-use guest launch. No Google session is granted to the test agent.

The Library avatar strip offers Join for each friend’s joinable game. Its overflow
menu includes the remaining friends. Both use the same targeted launch intent as
the game-card menu; an ended session shows an error rather than creating a new room.
