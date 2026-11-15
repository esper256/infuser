# On-demand managed server sessions

**Experimental v1; not an SDK 1.0 guarantee.** This contract covers dedicated and
persistent-world backends, plus single-player modes that use a shared server.
WebRTC peer hosting continues to use `peers.join()` / the peer session's `leave()`.

The game owns mode selection. Opening its title screen must not consume a server
reservation. Choosing a mode that needs a backend acquires one player lease on
the game's single shared server. Returning to a client-only mode releases that
lease, preserving identity, guest progress and cloud saves. When all leases end,
the existing idle timeout shuts the server down and releases its resource budget.
Persistent-world disk data survives that shutdown; this feature does not promise
an always-running world simulation.

## Publishing and compatibility

Check `GET /api/publisher/capabilities` for `runtime_sessions.version === 1`.
In `infuser.game.json`, alongside the existing managed `server` settings, add:

```json
{"server_start":"on_demand"}
```

The CLI validates and copies this into the immutable build manifest. It is only
valid for a game with a managed backend. Peer-hosted and client-only games omit
it. Builds without the field retain their existing launch-time warming behavior;
no existing game needs to republish. Explicit legacy launch `mode` remains
accepted, but never implicitly acquires an opted-in build's server.

The game checks `(await ready).capabilities.runtime_sessions?.version === 1`
before using this experimental API. Its initial `runtime.state` is `ready` (the
browser can play); `runtime.server.state` is independently `idle`. An unrelated
game consuming the server's resource budget does not block this title screen.

The declaration lives beside the legacy catalog, so an older daemon can still
read/rewrite that catalog without deleting it. Rolling back to a daemon without
runtime v1 restores its old warm-on-launch behavior; a new game's capability
check must show that this feature requires a newer Infuser. Binary rollback does
not provide missing SDK methods. Upgrading again restores the declaration.

## Game integration

```js
import { ready, runtime } from '/_infuser/sdk.js';
const context = await ready;
if (context.capabilities.runtime_sessions?.version !== 1) {
  throw new Error('This game needs a newer Infuser.');
}

// Call only when the player chooses a mode that needs the managed backend.
let session = await runtime.acquire();
// While state is "starting", keep the title/loading screen visible and poll
// runtime.status() about once a second. Use a bounded wait and offer Cancel.
// Connect to session.endpoint only once state is "ready".

// When returning to a client-only mode (or cancelling startup):
await runtime.release(session.session_id);
// Do not call Infuser.leave(): that ends this launch and its guest saves.
```

`acquire()` returns immediately with `starting` or `ready`; it does not wait for
the process or a network connection. Preserve its `session_id`. The browser SDK's
normal heartbeat renews an attached backend lease while also renewing the launch.
Check status during startup and when a game connection fails. A disconnected or
expired backend never auto-rejoins: return visibly to the game's title screen
and let the player retry. The game's own transport handles disconnecting sockets
when leaving multiplayer; releasing a lease is not a socket teardown protocol.

The game must bound its own loading wait. A request can fail after the server
accepted it: call `status()` to recover the attached session and release it if
the player cancelled. Repeating `acquire()` before release adopts the same live
lease. Do not retry an old acquire after the player has released/cancelled it.
A stopped game heartbeat eventually expires an abandoned reservation.

## Wire contract

Requests use the launch's HttpOnly cookie (or scoped game bearer ticket). Mutating
requests require the exact game HTTPS Origin. Player/app/build identity is derived
from the launch, never supplied in a body. No controller token is exposed.

| Request | Result |
| --- | --- |
| `GET /_infuser/runtime/v1` | Read this launch's server state; does not acquire or renew |
| `POST /_infuser/runtime/v1` | Acquire/reuse this launch's lease; 202 starting, 200 ready |
| `DELETE /_infuser/runtime/v1/SESSION_ID` | Release only this launch's matching lease; 200 with current state |

Example ready result:

```json
{
  "state": "ready",
  "session_id": "c91ab4f0000000000000000000000000",
  "endpoint": {"url": "https://g-EXAMPLE.games.example.com/_infuser/server"},
  "error": null,
  "retry_at": 0
}
```

`session_id` identifies the player's lease, not a separate world. Another player
gets another lease on the same server. HTTP/WebSocket endpoints use the player's
authenticated game origin; direct TCP/UDP backends return host, port and protocols.
The server must validate the scoped player ticket as before.

States are `idle`, `starting`, `ready`, `stopping`, `failed`, or `ended`. Only
`ready` has a usable endpoint. `idle` has no session ID; `ended` retains the old ID
until explicit release or acquisition. An expired backend does not invalidate
the browser's context, heartbeat, or saves. `error` is displayable explanatory
text, not a machine-readable code; `retry_at`, when present, is Unix seconds.

Repeated release is harmless. Releasing an old ID after reacquisition returns the
current state without ending the new lease. Supplying another player's ID never
releases their lease. Two tabs sharing the same signed-in build origin also share
its launch cookie/lease; independent guest launches have distinct leases.

Errors retain the existing `{ "error": "message" }` envelope: 401 expired/missing
launch, 403 membership/origin rejection, 422 build did not opt in, 409 stale build
or server stopping, 429 capacity in player leases, 503 resource budget or failure
backoff. Show a retry/reopen action instead of looping forever or making a second
world. A heartbeat is not permission to silently reacquire after an error.

Publication and acquisition are serialized. While the server is active, uploads
queue; activation waits for confirmed shutdown. After activation, an already-open
old title screen keeps its assets and saves but receives 409 if it tries to acquire
the new build's backend. It must reopen from the Library.
