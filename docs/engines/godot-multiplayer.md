# Godot multiplayer: experimental web study

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
- Forced fallback test: `?autoplay=1&multiplayer=1&relay=1`. The client explicitly
  requests relay-only ICE. Deterministic movement generates real RPC traffic
  without a model or tokens.
- One shared session: the first arrival hosts; the other joins the same epoch.
  Host departure ends the session. The addon fences RPCs when its lease expires.
  Brief signaling interruptions may recover within the lease; a failed link
  eventually returns to the title screen for explicit rejoining.
- `tests/godot_relay_journey.mjs` verifies relay/relay selected candidate pairs,
  traffic and real RPC reception in both directions, plus host departure. A second
  automatic-mode run verified a direct connection. Test-only instrumentation
  collects stats; the game has no numeric network overlay.

This is a two-player prototype, not the stable Godot SDK. It does not implement
Join friend intents, a reusable conflict chooser, native clients or Godot-specific
adult telemetry. Test long-running TURN credential rotation and background/suspend
behavior before promoting the recipe. No new production auth mode was introduced.

Other engine paths remain studies:

- **WebSocket + dedicated Godot server:** the controller and authenticated proxy
  exist; ARM64 headless packaging, readiness, admission and shutdown need a real
  Godot journey. Supply a managed recipe rather than asking children for Docker
  configuration.
- **ENet/UDP:** native clients only. A gated UDP port does not make peer-hosted
  ENet relay-capable. Add a deliberate relay adapter or use a dedicated server.
- **Native WebRTC:** bundle Godot's WebRTC extension for each OS/architecture,
  then test it with the web game. Native launch authentication is pending.
  Windows and Bazzite Linux are required acceptance platforms.

References: [Godot WebRTC](https://docs.godotengine.org/en/stable/tutorials/networking/webrtc.html),
[WebRTCMultiplayerPeer](https://docs.godotengine.org/en/stable/classes/class_webrtcmultiplayerpeer.html).
