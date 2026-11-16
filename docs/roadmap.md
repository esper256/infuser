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
- Compile and play the [Unity Web reference project](../examples/unity-web/README.md)
  with Unity 6.3 LTS. Verify C#/IL2CPP callbacks, stripping, guest saves, two-client
  forced relay, and a real user's export before upgrading its experimental status.
  Repeat with Brotli/fallback and any threaded export actually used by a game.
- Finish [Infuser Play](engines/godot-native.md) acceptance on physical Windows,
  including installation, browser-login handoff, cache cleanup and guest saves.
  Keep native support limited to the documented single-player preview meanwhile.
- Playtest an independently authored dedicated game with [dedicated sessions](dedicated-sessions.md) and test
  the [PeerJS adapter](peerjs.md) against an existing user's game. Platform fixtures
  and source review do not prove those games' integrations.
- Add a Windows publisher executable. Keep author checks usable with prebuilt
  tools or the game's existing runtime; do not require the contributor toolchain.
  Evaluate disposable hosted checks without sharing a user's Google login.

Preserve published games and saves during these checks. Firefly Drift's listed
active build must not be replaced merely to rerun a fixture.

## Design before expanding

- Build profile selection on the [identity foundation](proposals/identity-foundation.md):
  one account may have separate profiles in different groups, with an explicit
  chooser and full reload when switching. Keep credentials bound to one profile.
  Add other major sign-in providers only with explicit authenticated account linking;
  shared email addresses must not silently merge accounts.
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
