# Infuser Unity Web adapter

Audience: game authors and their agents.

**Start with the [Unity recipe](../../docs/engines/unity-web.md).** It covers package
installation, exports, manifests, saves, multiplayer, and publication. For a new
game, start with the [creation guide](../../docs/create-game.md); for an existing
game, follow the [publishing checklist](../../docs/agents.md). This page is the
C# reference; return to the guide that sent you here when finished.

This adapter targets Unity 6 and SDK/API major 1, both still experimental in
Infuser. C# checks with Unity test doubles and JavaScript/platform tests pass.
Real Unity import, IL2CPP export, and compiled-game playtests remain unverified.
Keep the installed revision with your game and update deliberately. No Infuser
build tools are needed to use it.

## Lifecycle

Use `InfuserClient.Instance` from Unity's main thread. The receiver survives scene
changes; do not rename or destroy it during play. Subscribe to events before
joining and unsubscribe when the owning scene/controller is destroyed. Deliberate
leave uses `LeaveMultiplayerAsync()`, not destruction of the singleton.

`await client.ReadyAsync()` returns `Identity`: `hosted`, `guest`, `app_id`,
`build_id`, `player_id`, `name`, and `library_url`. Outside Infuser, including the
Editor, `hosted` is false. Use the game's separate standalone saves/networking.
A hosted authentication error remains an error; never fall back to owner saves
or public matchmaking. No Google login or publishing credential belongs in a game.

`InfuserException` exposes `Code` and HTTP `Status` when available. Catch errors
at your UI boundary and offer retry or return to the Library. A timeout is not
proof that a save failed to commit; retain the checkpoint and retry it.

## Saves

`JsonSave<T>(client, group, file, validate)` supports one declared JSON file per
save group. `T` is a `[Serializable]` class with public fields. The optional
`Action<T>` validator checks schema/ranges on load, before saving, and before
committing a conflict choice; throw to reject unsupported data.

| Member | Behavior |
| --- | --- |
| `LoadAsync()` | Load before gameplay. `Value` is null when there is no checkpoint. |
| `SaveAsync(value, summary)` | Serialize once and retain an immutable checkpoint until acknowledged. Summary: up to 80 characters, no control characters. |
| `HasPending`, `RetryAsync()` | Retry the exact checkpoint after a failed request. Do not replace it with later progress. |
| `HasConflict`, `ReadConflictAsync(device)` | Read cloud (`false`) and this-session (`true`) copies to build a chooser. |
| `ResolveAsync(useDevice)` | Commit the selected copy against the observed cloud revision. Another concurrent write can require another choice. |

Only `SaveResult.status == "saved"` confirms saving. Pending data is in memory;
keep the page open during retry. There is no durable offline journal. For
multi-file/binary groups, use `CallJsonAsync` with `cloud.load`, `cloud.read`,
`cloud.history`, `cloud.commit`, or `cloud.resolve` and the
[checkpoint contract](../../docs/cloud-save-design.md). Guest data is already
scoped by Infuser; never copy signed-in progress into it.

## Presence and launch intent

`GetPresenceAsync()` returns `PresenceSnapshot`: players, `peer_sessions`, dedicated `sessions`,
`can_create`, and `unavailable_reason`. The legacy `session` and `can_start` remain available. A missing peer room
is normalized to null. Treat absent arrays as empty. Poll about every five seconds
only while needed; presence does not reserve a seat. `PresenceAsync()` remains
available for callers using raw JSON. See [presence](../../docs/presence.md).

`ConsumeLaunchIntentAsync()` returns `LaunchRequest`. Ordinary Play has a null
`intent`; let the user choose a mode. For `join_friend`, pass that exact intent
to the selected transport. On rejection, show the failure; do not retry without
the target or create a new room. See [launch intents](../../docs/launch-intents.md).

## Peer-hosted games

Use `CreatePeerRoomAsync()` to host an independent joinable room on Play.
Pass `joinable: false` to start closed while loading.
`JoinPeersAsync(PeerOptions)` accepts `intent`, optional `sessionId`, `relayOnly`,
and `joinable`. The default is closed; the host calls `SetJoinableAsync(true)`
after initializing. `PeerRoom` contains the connection `id`, `hostId`, `role`,
and room `epoch`. Peer IDs are transport identifiers, not stable player IDs.

- `PeerStateChanged(PeerState)`: role, peers, connected peer IDs, and joinability.
- `PeerMessageReceived(PeerMessage)`: sender `from` and the game's serialized `json`.
- `PeerEnded(NetworkEnd)`: return to the title screen or explicitly rejoin.
- `SendPeerAsync(to, message)`: returns `.sent`; false means send the latest state
  later rather than accumulating a queue.
- `BroadcastAsync(message)`: best effort to connected peers; it is not an
  acknowledgement that every player received the message.

Validate game messages. The host owns shared simulation. Host departure ends the
session. There is no automatic scene replication, RPC mapping, or host migration.
The [peer contract](../../docs/peer-hosting.md) requires TURN fallback; test with
`relayOnly = true` before claiming peer multiplayer works.

## Dedicated WebSocket games

Use the [dedicated-session contract](../../docs/dedicated-sessions.md) and
[Node server recipe](../../docs/engines/node-server.md) for backend integration.
`ConnectSessionAsync(SessionOptions)` returns `SessionConnection`, including
`session.id`, `attempt_id`, and `lease_id` after admission. Options:

| Intent | Options |
| --- | --- |
| Join the shared multiplayer game, creating only if none exists | Defaults: `action = "play"`, `activity = "multiplayer"` |
| Single-player server run | `activity = "solo"` |
| Explicit separate multiplayer run | `action = "create"`, if the manifest allows multiple sessions |
| Join a known room | `action = "join"`, `target = new SessionTarget { session_id = observedId }` |
| Join Friend | `intent = launch.intent`; keep its session and player target |
| Resume after transport loss | `action = "resume"`, target the original session ID |

Set `timeoutMs` if needed; the browser SDK bounds it to 1–90 seconds. Do not create
a replacement session when a targeted join/resume fails.

Subscribe before connecting: `SocketMessageReceived(SocketMessage)` provides
`IsBinary`, `Bytes`, or `text`; `SocketEnded(NetworkEnd)` provides code/reason.
`SendTextAsync(text)` and `SendBytesAsync(bytes)` return `.sent`. Keep payloads
within the game's declared connection limits and handle backpressure.

Unexpected connection loss keeps the short resume reservation. Deliberate
`LeaveMultiplayerAsync()` cancels admission, closes networking, and releases the
runtime lease, including after a disconnected screen. It does not end the launch
or close the game. Listen to `NetworkFailed(NetworkError)` for socket or hosted
session failures.

## Compatibility escape hatch

Existing `Event(name, json)`, `CallJsonAsync(operation, arguments)`, and
`CallAsync<T>` remain available. Events are `peer_state`, `peer_message`,
`peer_end`, `socket_message`, `socket_end`, `socket_error`, and `session_error`.
The typed helpers wrap these operations without changing the platform protocol.
Do not handle both typed and raw events for the same gameplay action.

For adapter development and acceptance checks, see the separate
[Infuser developer guide](../../docs/development.md#unity-adapter-checks).
