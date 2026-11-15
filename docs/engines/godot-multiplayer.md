# Godot multiplayer: next targeted study

First determine whether this existing game uses WebSocket, WebRTC, or ENet.
Preserve its solo support independently of its multiplayer topology.

- **WebSocket + dedicated Godot server:** the first planned managed recipe.
  Infuser's controller and authenticated WebSocket proxy are available. The
  engine-specific ARM64 headless packaging, readiness, ticket validation and
  shutdown adapter still need a Godot dogfood journey before being called turnkey.
- **WebRTC peer host:** blocked until Infuser signaling and relay fallback are
  installed. Direct-only peer hosting is not a supported substitute.
- **ENet/UDP:** native clients only. Infuser can allocate a gated UDP port, but the
  relay/native launcher and engine admission recipe are not complete.

Agents should ask the engine-specific question they cannot infer from code,
not ask the child to invent Docker configuration. The final recipe should supply
resource limits, readiness, persistence and shutdown behavior itself. Solo and
multiplayer clients must join the same active shared world when appropriate.
