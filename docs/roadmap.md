# Remaining work

Audience: Infuser developers.

This page lists unfinished work. Use [feature tests](developer-testing.md) for
acceptance evidence and limits, and the linked contracts for implemented behavior.
Keep individual deployments' configuration and status in private operator records.

## Stabilize the current platform

- Complete the [SDK 1.0 stabilization plan](proposals/sdk-1.0-stabilization.md).
  Freeze the candidate contract and versioned clients before declaring stability.
  Major-version admission exists; the overall contract remains a preview.
- Exercise [visits](proposals/temporary-visits.md) with real groups before freezing
  ownership and access semantics. Keep unsupported direct-port servers out of visits.
- Finish [Godot web](engines/godot-web.md) checks on Windows browsers, physical
  laptop sleep and full-duration TURN renewal. Accelerated recovery tests do not
  replace these checks.
- Finish [Infuser Play](engines/godot-native.md) acceptance on physical Windows,
  including installation, browser-login handoff, cache cleanup and guest saves.
  Keep native support limited to the documented single-player preview meanwhile.
- Playtest Lanternfall with [dedicated sessions](dedicated-sessions.md) and test
  the [PeerJS adapter](peerjs.md) against an existing user's game. Platform fixtures
  and source review do not prove those games' integrations.
- Add a Windows publisher executable. Keep author checks usable with prebuilt
  tools or the game's existing runtime; do not require the contributor toolchain.
  Evaluate disposable hosted checks without sharing a user's Google login.

Preserve published games and saves during these checks. Firefly Drift's listed
active build must not be replaced merely to rerun a fixture.

## Design before expanding

- Design direct messages, custom chats, temporary channels and notification
  preferences on the [channel foundations](proposals/chat-channels.md) before
  exposing new UI or APIs. Preserve visit access windows and host history.
- Keep game names and URL slugs independent of stable IDs. Future rename flows
  must preserve saves, credentials, visit progress and active launches, using
  aliases or redirects where needed.
- Expand engine-server recipes after the Node path is solid. Native multiplayer
  needs a separate study; Bevy remains paused.
- Review achievements before introducing persistent IDs or game-facing behavior.

Use measurements before revisiting the catalog, authentication or Docker-client
rewrites deferred by the [architectural review](proposals/architecture-improvement-plan.md).
Per-project resources and backup policies can be settled when another project is
added to a shared host; they are not prerequisites for the current platform.
