# Godot multiplayer: web preview

Determine whether the existing game uses WebRTC, WebSocket or ENet. Single-player
support is independent of multiplayer topology. Preserve the game's architecture
unless adapting it is an explicit integration decision.

The [Lantern Lake example](../../examples/godot-web/README.md) exercises two Godot
web clients through Infuser signaling and TURN. Its addon creates an actual
`WebRTCMultiplayerPeer`; Godot creates reliable/unreliable/ordered channels and
exchanges `Vector2` movement using GDScript RPCs. Game packets do not go through
Firefly's JSON messaging helper.

- Normal play: `direct_connect: optional`, `peers.join(false)`. ICE prioritizes
  LAN host candidates, direct internet candidates, then relays, using connectivity
  checks. TURN remains mandatory and available. No UPnP required.
- Lantern Lake fixture only: `?autoplay=1&multiplayer=1&relay=1`. The client explicitly
  requests relay-only ICE. Deterministic movement generates real RPC traffic
  without a model or tokens.
- One shared session: the first arrival hosts; the other joins the same epoch.
  Host departure ends the session. The addon fences RPCs when its lease expires.
  Brief signaling interruptions retry within the lease; a failed transport is
  rebuilt within the same room. Godot's member MultiplayerPeer is recreated when
  reconnecting to the host so polling and RPCs resume. Expiry ends the room.
- `tests/godot_relay_journey.mjs` verifies relay/relay selected candidate pairs,
  traffic and real RPC reception in both directions, plus host departure. A second
  automatic-mode run verified a direct connection. Test-only instrumentation
  collects stats; the game has no numeric network overlay.

This is a two-player prototype, not the stable Godot SDK. Join Friend is explicit
and targeted; consuming an intent never silently creates a replacement room.
The web addon renews TURN ahead of its advertised deadline, retries transient
failures and refreshes connections. The recovery journey passed on Bazzite's
Flatpak Chrome with both threaded and single-threaded exports. It accelerates the
renewal deadline while using real TURN credentials; it does not claim a full-hour
soak or physical suspend test. Native multiplayer remains a separate study.

The following paths are not validated Godot integrations:

- **WebSocket + dedicated Godot server:** the controller and authenticated proxy
  exist; ARM64 headless packaging, readiness, admission and shutdown need a real
  Godot journey. Supply a managed recipe rather than asking children for Docker
  configuration.
- **ENet/UDP:** native clients only. A gated UDP port does not make peer-hosted
  ENet relay-capable. Add a deliberate relay adapter or use a dedicated server.
- **Native WebRTC:** bundle Godot's WebRTC extension for each OS/architecture,
  then test it with the web game. The desktop preview currently permits only
  client-only single-player games.
  Windows and Bazzite Linux are required acceptance platforms.

References: [Godot WebRTC](https://docs.godotengine.org/en/stable/tutorials/networking/webrtc.html),
[WebRTCMultiplayerPeer](https://docs.godotengine.org/en/stable/classes/class_webrtcmultiplayerpeer.html).
