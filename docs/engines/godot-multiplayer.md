# Godot web multiplayer

Audience: game authors and their agents.

Use this alongside the [Godot web recipe](godot-web.md). For a new game, read it
for networking design and return to the [creation guide](../create-game.md);
credentials are not needed yet. When integrating for publication, return to
[Explicit play modes](../agents.md#explicit-play-modes) after adapting multiplayer,
then finish the main checklist's saves, presentation, publication, and handoff.

This preview supports two-player Godot web games using WebRTC. First follow the
[web export guide](godot-web.md). Keep the game's simulation and RPCs in GDScript;
the [example addon](../../examples/godot-web/addons/infuser) provides Infuser
connection setup and creates a Godot `WebRTCMultiplayerPeer`.

Check whether the existing game uses WebRTC, WebSocket, or ENet before adapting
it. These transports are not interchangeable. Do not claim support for an existing
networking stack merely because the game exports to the web.

## Connect and leave

Use the peer-hosted WebRTC declaration in the
[example manifest](../../examples/godot-web/infuser.game.json), keeping only the
play modes the game actually supports. The installation must advertise
`peers.webrtc.v1` and `peers.rooms.v1`. Include both in `sdk.required_capabilities`.

Use `direct_connect: "optional"` and `await peers.create()` on the title screen's Play
button. Copy the current addon when updating an existing game. Leave Play Solo
unchecked by default. When checked, skip peer setup and start local play.
WebRTC checks local-network and direct internet paths as well as TURN relay paths;
relay fallback is required even when direct connections work. UPnP is not needed.
Use `peers.create(true)` on the host and `peers.join(true, intent)` on the joining player to test relay.

Each host creates a separate room. A friend joins the selected host. Host departure or an
expired connection lease ends the room. The addon retries brief interruptions,
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
