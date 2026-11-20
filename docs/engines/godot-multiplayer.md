# Godot web multiplayer

Audience: game authors and their agents.

Use this alongside the [Godot web recipe](godot-web.md). For a new game, read it
for networking design and return to the [creation guide](../create-game.md);
credentials are not needed yet. When integrating for publication, return to
[Explicit play modes](../agents.md#explicit-play-modes) after adapting multiplayer,
then finish the main checklist's saves, presentation, publication, and handoff.

This preview supports Godot web games for 2–8 players using WebRTC. Set
`multiplayer.max_players` to any integer in that range. The platform refuses a
larger peer-hosted room. First follow the [web export guide](godot-web.md). Keep
the game's simulation and RPCs in GDScript; the
[example addon](../../examples/godot-web/addons/infuser) provides Infuser
connection setup and creates a Godot `WebRTCMultiplayerPeer`.

Check whether the existing game uses WebRTC, WebSocket, or ENet before adapting
it. These transports are not interchangeable. Do not claim support for an existing
networking stack merely because the game exports to the web.

## How the room is shaped

The addon uses a star. The host calls `create_server()` and is Godot peer id 1.
Each member calls `create_client()` with their own id. Those ids start at 2 and
come from a hash of the coordinator id, so each member's id is stable for the
session and different from the others. Each member's only WebRTC connection is
to the host. The addon turns `server_relay` on, and Godot forwards RPCs through
the host, so a normal `rpc()` from one member reaches the others.

Bandwidth follows that star. Every member relays via the host, and through TURN
when that path is the one ICE selects. The host uploads one copy of each
relayed packet per other player. A full room is seven WebRTC connections on the
host. Keep packets small and steady. Lantern Lake sends positions about ten
times a second.

The coordinator delivers signaling only between the host and each member.
A member who leaves frees a seat immediately. A room at `max_players` is not
joinable; presence reports the reason `full`. The next join can take the freed
seat. The host retries one member whose WebRTC link fails and keeps the others
in the room. A member who cannot reach the host leaves, which frees that seat.
Losing the host, or the host's lease, ends the room for everyone.

## Upgrading a two-player game

Copy the current addon over `addons/infuser`. Then:

1. In `infuser.game.json`, set `multiplayer.max_players` to the party size, from
   2 through 8. Keep `mode` `peer_hosted`, `transport` `webrtc`, and
   `relay_fallback` `required`. Republish with the same app id.
2. Store remote state in a `Dictionary` keyed by
   `multiplayer.get_remote_sender_id()`. A single `Vector2`, a single remote
   node, or a literal peer id of 2 only has room for one friend.
3. Keep sending with `rpc()` or `rpc_id()`. The host relay delivers a member's
   call to the other members. `add_peer` is only for the host connection. Leave
   the addon on `create_server` / `create_client` with `server_relay` enabled.
4. Connect `multiplayer.peer_disconnected` and delete that player's nodes. The
   host's leave on the server frees the seat.
5. Leave Play on `peers.create()` and Join Friend on `peers.join(false, intent)`.
   Play Solo stays local and does not create a room.

Lantern Lake (`examples/godot-web`) is the pattern: `max_players` is 4, and each
joined player has their own lantern.

## Connect and leave

Use the peer-hosted WebRTC declaration in the
[example manifest](../../examples/godot-web/infuser.game.json), keeping only the
play modes the game actually supports. The installation must advertise
`peers.webrtc.v1` and `peers.rooms.v1`. Include both in `sdk.required_capabilities`.

Use `direct_connect: "optional"` and `await peers.create()` on the title screen's Play
button. Copy the current addon when updating an existing game. Leave Play Solo
unchecked by default. When checked, skip peer setup and start local play. If
upgrading an older game, replace `await peers.join()` in its host handler.
Use `can_create` for host availability, even when `peer_sessions` contains
other rooms. Keep the targeted `join` call in the Join Friend handler.
WebRTC checks local-network and direct internet paths as well as TURN relay paths;
relay fallback is required even when direct connections work. UPnP is not needed.
Use `peers.create(true)` on the host and `peers.join(true, intent)` on each joining player to test relay.

Each host creates a separate room. A friend joins the selected host. Host departure or expiry of the host’s
lease ends only that room. A departing member frees a seat. The addon retries brief interruptions,
refreshes TURN credentials, and reconnects within the same live room. After a room
ends, offer an explicit return to the title screen or a new join; do not resume an
old simulation automatically.

For Join Friend, follow the [web guide](godot-web.md): consume the launch intent
once and pass it to `peers.join(false, intent)`. If that session or friend is
gone, show the title screen with an explanation. Never create a replacement room.

Use the game's existing checks and the [publication checks](../agents.md#publish-and-verify).
With an authorized player session, check actual RPC traffic in both directions,
forced relay, reconnects, guest isolation, and host departure. If no such session
is available, report live checks as untested and hand the published game to its player.
Physical laptop sleep and Windows browser checks remain unverified for this preview.

## Unsupported paths

Dedicated Godot WebSocket servers, ENet/UDP, and native WebRTC do not yet have
supported Infuser integration recipes. Native Godot currently supports only
client-only single player. Report a missing integration through the
[agent guide](../agents.md#reporting-platform-problems); do not remove multiplayer
just to make publication succeed.

References: [Godot WebRTC](https://docs.godotengine.org/en/stable/tutorials/networking/webrtc.html),
[WebRTCMultiplayerPeer](https://docs.godotengine.org/en/stable/classes/class_webrtcmultiplayerpeer.html).
