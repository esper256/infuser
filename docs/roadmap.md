# Next steps

- Before freezing SDK 1.0, exercise [Godot web](engines/godot-web.md) with the
  hidden Lantern Lake test game: real GDScript saves, guest isolation, and Godot
  RPCs over forced TURN. Bazzite Flatpak Chrome now passes single/threaded recovery
  and Join Friend journeys. Existing upstream Dodge the Creeps passes adapted
  gameplay, saves and guest isolation. Finish actual Windows browser acceptance,
  physical laptop sleep and a full-duration TURN soak before calling these complete.
- Exercise the [Rust desktop preview](engines/godot-native.md): browser account
  handoff, verified immutable downloads, native cloud saves and isolated guest
  directories. Native multiplayer remains paused; Windows hardware acceptance
  follows the installer handoff. Keep native delivery separate from SDK 1.0.
- Review the [game SDK 1.0 contract](sdk-contract.md) after these engine studies
  expose browser-specific assumptions. Major 1 remains preview. Firefly is now
  listed by the operator's request; preserve its active build.
- Dogfood experimental [peer-hosted WebRTC](peer-hosting.md) in Firefly first:
  forced TURN, two autoplay clients, host departure/rejoin, connection recovery,
  and queued updates. Keep multiplayer protocol changes experimental until review.
  Fast SDK lifecycle journeys now cover recovery, delayed negotiation after
  leaving, obsolete channels, and lease expiry. Live Firefly verified that two
  relay-only players keep the old build while an update queues, host departure
  activates the update, and old clients must reopen before multiplayer. A fresh
  launch receives the new build. [SDK major/capability declarations and the frozen
  old-client journey](sdk-versioning.md) are now implemented; major 1 stays preview.
- Dogfood the implemented [declared save-file snapshot model](cloud-save-design.md),
  including durable conflict candidates and game-owned resolution. The legacy
  slot API retains its original behavior; new checkpoints use separate v1 routes.
- On-demand managed server acquisition/release is implemented as an opt-in build
  contract; see [runtime sessions](runtime-sessions.md). Legacy builds still warm
  on launch. [In-game presence and explicit peer joinability](presence.md) are
  implemented; dogfood title-screen discovery, host admission controls and stale
  targeted joins in Firefly. [Join friend launch intents](launch-intents.md) are
  implemented behind the card dropdown; online-player data also supports future
  avatar actions. Review server-authenticated joinability separately for managed
  backends. Keep secondary actions off the future game hero art.
- [Readable game URLs](game-urls.md) are implemented: stable entry links, retained
  rename aliases, collision/reserved-name handling, and named isolated play
  origins. Guest and build isolation remain unchanged.
- Add managed engine server packaging after the first engine's journey stabilizes.
- Review achievements separately before implementing their durable API.
