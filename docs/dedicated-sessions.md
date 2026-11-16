# Dedicated multiplayer sessions

Audience: game authors and their agents.

Use this with the Node hosting recipe during [step 2 of the agent guide](agents.md#choose-the-integration).
It connects the game's existing room logic to Infuser's players and Join Friend.
After integration, return to [Explicit play modes](agents.md#explicit-play-modes)
and continue through saves, presentation, publication, and handoff.

SDK v1 candidate for browser games using Node `ws`. One managed process can host
one shared multiplayer run or several explicitly created runs. Infuser handles
verified player identity, discovery, cold start and connection lifetime. The game
keeps its room rules, simulation, saves and text/binary gameplay messages.

Check that the installation advertises both `sessions.v1` and
`connections.websocket.v1` in `sdk.capabilities`. Older builds and their routes
keep working. Other transports need their own adapter; this is not Socket.IO,
WebRTC, raw TCP or UDP support.

## Declare the game's behavior

Merge these fields into the [Node recipe](engines/node-server.md):

```json
{
  "server_start": "on_demand",
  "sessions": {"policy": "multiple", "max_players_per_session": 8},
  "server_connection": {"path": "/", "protocols": [], "max_message_bytes": 65536},
  "launch_intents": ["join_friend"],
  "sdk": {"major": 1, "api_major": 1, "required_capabilities": ["sessions.v1", "connections.websocket.v1"]}
}
```

Preserve the other required SDK capabilities. `shared` allows one multiplayer
session per game; `multiple` also allows an explicit “Start a separate run” choice.
Both allow independent server-backed single-player runs. Declare `solo_runtime:
"shared_server"` if single player is supported by this path. The existing
`multiplayer.max_players` must match `max_players_per_session` (2–256 for multiplayer;
1 is allowed for a single-player-only server). Capacity
is per gameplay session, not the total server process. The effective process and
connection limits come from `capabilities.sessions`; show capacity errors and
never try to bypass them with a second container.

Games with browser-only single player can keep `solo_runtime: "client_only"`
alongside dedicated multiplayer. That mode uses no server or `sessions.connect()`;
only multiplayer enters the dedicated adapter. Do not move single-player gameplay
onto a server just to integrate multiplayer.

`server_connection` is optional: `/`, no subprotocol and 64 KiB are the defaults.
Use a fixed backend path and up to eight WebSocket subprotocols; no query string.
The configured maximum message size may be 1 KiB–1 MiB; smaller messages are allowed.
These fields apply to the hosted transport, not the
standalone server's public URL. SDK configuration supplies the hosted URL.

## Browser: enter a run

Keep the existing title screen. Use the [optional helper](../sdk/optional.js) to
choose the hosted integration only when Infuser injected its SDK script. An
invalid hosted launch is an error, not a reason to switch to public matchmaking.

```js
const socket = await Infuser.sessions.connect({activity: 'multiplayer'});
socket.onmessage = event => handleGameMessage(event.data);
socket.onclose = () => showReconnectOrTitleScreen();
const sessionId = socket.infuser.session.id;
```

This warms the backend, requests admission, and returns an ordinary WebSocket
**after the game server accepts the player**. Early welcome frames are delivered
after the awaiting caller attaches handlers. Existing gameplay framing stays intact.
Options include `binaryType`, `signal` and `timeoutMs` (default 60 seconds, maximum
90). Preserve `socket.infuser.lease_id` for explicit release when returning to the
title screen. Failed startup has a bounded lease even if the caller disappears.

| Player choice | `sessions.connect()` options |
| --- | --- |
| Multiplayer | `{activity:'multiplayer'}` — joins an existing joinable run; creates a run only if no multiplayer run exists |
| Single player | `{activity:'solo'}` |
| Separate multiplayer run | `{action:'create',activity:'multiplayer'}` — requires `multiple` while another run exists |
| Join a displayed run | `{action:'join',target:{session_id,player_id}}` — player ID is optional |
| Resume this launch's disconnected run | `{action:'resume',target:{session_id}}` |
| Launcher Join Friend | `{intent:(await Infuser.launchIntent.consume()).intent}` — only when an intent was returned |

If multiplayer runs exist but none can accept this player, ordinary Multiplayer
fails visibly. It does not create another run. A separate run requires the
explicit `create` action and the game's policy must allow it.

An exact join or resume never creates a replacement world. Catch `error.code`:
`session_ended`, `session_full`, `friend_left`, `session_unavailable`,
`server_draining`, `capacity_unavailable`, `resume_expired`, `already_connected`,
`session_exists` or `admission_rejected`. Infrastructure failures also have codes
such as `backend_unavailable`, `connection_timeout` and `connection_lost`.
Refresh the choices and let the player decide. Do not retry as an untargeted join.

To leave deliberately:

```js
await Infuser.sessions.disconnect(socket);
await Infuser.runtime.release(socket.infuser.lease_id);
```

This preserves the browser launch and cloud saves. Avoid `Infuser.leave()` unless
ending that launch too. Closing with WebSocket code 1000 releases the game seat;
an unexpected disconnect holds a seat for up to 30 seconds. A fresh launch cannot
resume another launch's reservation. Signing out of the website leaves an already
running game connected. Disabling membership or moving private groups still ends
access. Background tabs can stay connected without relying on JavaScript timers.

## Node: reserve, attach, release

Copy [hosting.mjs](../sdk/server/hosting.mjs) and
[connection.mjs](../sdk/server/connection.mjs) together into the server source.
They use Node built-ins; the game supplies its existing `ws` dependency. The
publisher packages these files with the game. There is no global SDK install.

```js
import {WebSocketServer} from 'ws';
import {createInfuserServer, AdmissionError, sessionId} from './infuser/hosting.mjs';

const sockets = new WebSocketServer({noServer: true, maxPayload: 65536});
const hosting = createInfuserServer({
  server: httpServer,
  sockets,
  sessions: () => listRoomDescriptors(),
  admit: async ({identity, request, signal}) => {
    // Select/create exactly the requested room and reserve its seat atomically.
    // Throw new AdmissionError('session_full') etc. on refusal.
    const seat = await reserveSeat(request, identity.player_id, signal);
    return {
      session: seat.room.descriptor,
      attach: (socket, {identity, resumed}) => seat.attach(socket, identity, resumed),
      suspend: () => seat.pauseInput(),
      release: reason => seat.release(reason)
    };
  },
  onControl: state => showServerUpdatePending(state.state === 'draining')
});
if (!hosting) installStandaloneUpgradeHandler(httpServer, sockets);
```

The sketch uses game-owned functions. A complete runnable example is
[Dedicated Server Lab](../examples/dedicated-node/README.md). Each descriptor is
`{id,label,activity,joinable,max_players}`. Generate a new `sessionId()` for each
run; never reuse an ended run's ID. Use `solo` or `multiplayer` as the activity.
Only multiplayer runs should be joinable. Keep the label short and useful.

`identity` includes opaque `player_id`, display `name`, `guest`, `launch_id`,
`connection_id`, `app_id`, `build_id`, `instance_id` and `group_id`. Use the verified
identity supplied to the callback; do not accept an identity from gameplay JSON.
Guests have separate identities. Never use a display name as an account/save key.

The helper serializes admissions. Your callback must reserve a seat before it
returns, respect its abort signal, and finish within four seconds. `attach` may
run again on resume; replace the old socket without creating another player.
`release` must free that reservation even if attachment never happened. Do not
leave a new empty room behind after a rejected reservation. Other game-specific
entry paths must use the same atomic seat accounting.

Reports are full snapshots, sent every ten seconds and after admission or departure.
The helper supplies verified connection handles; games do not report player IDs or
spoof Library presence. Omit a room from `sessions()` once it ends, or call
`hosting.endSession(id)` to close its connections and release its reservations.
Change descriptors and call `hosting.report()` for immediate joinability updates.
Call `hosting.close()` during shutdown. Errors go to `onError` (console by default).

## Lifetime and updates

Infuser provides process credentials and refreshes short-lived server access tokens
through the helper. Nothing secret goes into the public web export. No publishing
credential belongs in a running game server. Standalone mode is selected only
when hosted configuration is absent; incomplete hosted configuration fails.

Discovery and server reports do not keep a container awake. An acquisition gets
a bounded startup lease; authenticated live sockets renew demand. Returning to
the title screen must release demand. Disappeared clients expire, and reports
older than 45 seconds cease to advertise sessions. A controller restart closes
transports; the helper registers again, reports current rooms and accepts an
explicit resume while its reservation still exists. Longer outages end the run
unless the game has its own persistent recovery mechanism.

A published update queues behind the active client/server pair. The helper sees
`draining`; new runs and joins are rejected, existing runs and bounded resumes
continue. When runs finish and the server stops, Infuser activates the update.
An old title screen must reopen from the Library to use the new backend. Do not
make a gameplay session immortal merely because a title-screen tab remains open.

## HTTP contract and debugging

Browser attempts use the game launch cookie and exact game origin:

- `GET /_infuser/api/v1/connections/v1` supplies coordinator epoch and connection settings.
- `PUT /_infuser/api/v1/connections/v1/ID` prepares a request. ID is the epoch plus
  32 random lowercase hex characters. Retry the same ID and body after a lost response.
- `GET .../ID` returns state, admitted session and a typed error outside the
  WebSocket handshake. `DELETE .../ID` cancels it. A consumed ID cannot open again.
- `GET .../ID/socket` upgrades exactly once. The SDK supplies the declared protocols.

Server routes use the provided instance credential for registration, then its
short-lived scoped bearer token: `POST /api/server/v1/register`,
`PUT /api/server/v1/sessions`, `POST /api/server/v1/connections/ID/admission`,
and `GET /api/server/v1/control`. The Node helper owns these calls, sequence
numbers, coordinator changes and credential refresh. Reports cannot authorize a
new player; admission is tied to an existing verified connection attempt.

For adapter authors, the JSON contract is:

1. Register with `Authorization: Bearer INFUSER_INSTANCE_TOKEN` and
   `{"app_id":"APP","instance_id":"INSTANCE"}`. The response contains `token`,
   `expires_at` (Unix seconds), `scope`, `coordinator`, `sessions` (the manifest
   declaration), `connection`, `limits` and `draining`. Scope has `app_id`, `build_id`,
   `instance_id`, `group_id` and `exp`, plus optional `deployment_id` for a borrowed
   visit deployment. `app_id` always identifies the original game. Infuser supplies
   `INFUSER_GAME_ID` unchanged and `INFUSER_DEPLOYMENT_ID` for the running deployment.
   Each deployment has its own process, volume, admissions and session directory;
   store world data in the supplied persistent directory. Never use an app ID or
   title alone to identify a world outside that directory. The server helper
   already registers with the correct instance; games need no visit-specific
   connection logic. Treat both credentials as opaque. The token
   lasts 120 seconds; register again before expiry or once after a 401.
2. Send snapshots with `Authorization: Bearer TOKEN`:

   ```json
   {
     "coordinator": "REGISTRATION_COORDINATOR",
     "seq": 42,
     "sessions": [{
       "id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
       "label": "Depth 3",
       "activity": "multiplayer",
       "joinable": true,
       "max_players": 8,
       "connections": ["VERIFIED_CONNECTION_ID"]
     }]
   }
   ```

   Sequence numbers are positive integers up to JavaScript's safe-integer limit.
   Increase them for each snapshot; retry the same body after a lost response.
   The result is `{accepted,seq,coordinator}`. Old/equal sequences return
   `accepted:false` and the latest sequence without renewing report freshness.
   `coordinator_restarted` means register again and send a new full snapshot with
   sequence 1. Omitted sessions end; their IDs cannot be reused in that process.
   Unknown/retired connection handles are ignored; a known connection cannot move
   to a room to which it was never admitted. The body limit is 128 KiB.
3. Before accepting the WebSocket upgrade, POST either
   `{"session":{"id":"...","label":"Depth 3","activity":"multiplayer","joinable":true,"max_players":8}}`
   or `{"error":"session_full"}` to that verified connection's admission route.
   The descriptor omits `connections`. The result is `{"accepted":true}`;
   identical retries succeed, conflicting results fail with 409. Success means
   Infuser accepted this admission decision; transport establishment still follows.
   Reserve the seat first and release it if admission or attachment fails.
4. Poll control about every ten seconds. It returns
   `{coordinator,instance_id,state,pending_build,reconnect_grace_seconds}`.
   `state` is `running`, `draining` or `stopping`. This is a current-state snapshot,
   not a consumable event queue; no cursor or acknowledgement is required.
   A retired process receives 401 and must not register as a newer instance.

The upgrade carries `X-Infuser-Connection`, an Ed25519 compact signed proof.
Its header is `{"alg":"EdDSA","typ":"infuser-connection+jwt","v":1}`. Verify
the signature with the base64url public key from `INFUSER_CONNECTION_PUBLIC_KEY`,
the installation issuer from `INFUSER_CONNECTION_ISSUER`, audience
`infuser-server-connection`, your app/instance, `iat`, `exp` and single use of
`connection_id`. Proofs last at most 60 seconds. The signed payload contains the
identity fields listed above and `request:{action,activity,target}`; target has
`session_id` and optional `player_id`. Keep gameplay bodies out of identity checks.
The Node helper implements this contract and the reconnect reservation lifecycle.

Use [publisher diagnostics](server-diagnostics.md) for process logs, startup/crash
details and recent attempts. Agent publication checks do not require Google login;
follow [the publishing verification guidance](agents.md#publish-and-verify).
