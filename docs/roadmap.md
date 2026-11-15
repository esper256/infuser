# Next steps

- Stay focused on Three.js/Vite until its complete journey is solid: publishing,
  updates, saves, guest isolation, sessions, and the SDK contract. Firefly remains
  a disposable preview during this period. Do not expand engine compatibility
  while these contracts are still changing.
- Review and stabilize the [game SDK 1.0 contract](sdk-contract.md) before real
  games depend on it. Then consider Godot and Bevy; their implementation is paused.
- Move server acquisition into the game so title screens need no running backend;
  expose presence and joinability; add opt-in Join friend launch intents.
- Give games a readable, unique URL slug. Preserve guest origin isolation and
  build isolation; a friendly entry address may differ from its actual play
  origin. Account for renames, collisions, reserved names, and old links.
- Add managed engine server packaging, then relay/signaling with mandatory
  fallback tests before advertising peer-hosted support.
- Review achievements separately before implementing their durable API.
