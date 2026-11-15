# Next steps

- Stay focused on Three.js/Vite until its complete journey is solid: publishing,
  updates, saves, guest isolation, sessions, and the SDK contract. Firefly remains
  a disposable preview during this period. Do not expand engine compatibility
  while these contracts are still changing.
- Review and stabilize the [game SDK 1.0 contract](sdk-contract.md) before real
  games depend on it. Then consider Godot and Bevy; their implementation is paused.
- Dogfood experimental [peer-hosted WebRTC](peer-hosting.md) in Firefly first:
  forced TURN, two autoplay clients, host departure/rejoin, connection recovery,
  and queued updates. Keep multiplayer protocol changes experimental until review.
- Then implement the reviewed [declared save-file snapshot model](cloud-save-design.md),
  including durable conflict candidates and game-owned resolution. The current
  slot API still rejects stale writes without storing the losing candidate.
- Move server acquisition into the game so title screens need no running backend;
  expose presence and joinability; add opt-in Join friend launch intents.
- Give games a readable, unique URL slug. Preserve guest origin isolation and
  build isolation; a friendly entry address may differ from its actual play
  origin. Account for renames, collisions, reserved names, and old links.
- Add managed engine server packaging after the first engine's journey stabilizes.
- Review achievements separately before implementing their durable API.
