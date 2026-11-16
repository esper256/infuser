# Remaining work

Audience: Infuser developers.

- Next: follow the [SDK 1.0 stabilization plan](proposals/sdk-1.0-stabilization.md).
  Record the candidate contract and freeze versioned clients before declaring any
  capability stable. The public binary-save helper exports are fixed and verified
  through the main SDK import. SDK major 1
  and capability admission exist; the overall contract remains a preview.
  Preserve published games and saves; Firefly Drift is listed and its active
  build must not be replaced merely to rerun a fixture.

- Setup deferred at the operator's request: off-server backup authorization,
  cloud snapshot policy, notification delivery, and an independent uptime probe.
  The [unattended operations](proposals/unattended-operations.md) implementation
  is deployed. ARM64 monitoring uses approximately 321 MiB combined; service limits
  and health checks were verified. Destination-specific acceptance remains open;
  local backup/restore fixtures do not prove that remote backups are configured.

- Follow up on the [architectural review results](proposals/architecture-improvement-plan.md):
  validate native cache cleanup on physical Windows hardware. Use measurements before taking on the
  deferred catalog, authentication or Docker-client rewrites.
- Make game-author tools portable: add a Windows publisher executable and provide
  optional integration checks that use prebuilt tools or the project's existing
  runtime. Keep the Cargo/Playwright contributor harness out of the publishing
  journey. Evaluate a disposable hosted test session for checks that need the real
  platform, without requiring the user's Google login or a local Infuser install.
- Integrate and playtest Lanternfall using [dedicated sessions](dedicated-sessions.md).
  The platform supplies verified identity, discovery, Join Friend, reconnects and
  queued updates. Shared and multiple-run fixtures exercise the generic contract;
  reviewing Lanternfall's source is not a playtest or an adaptation of that game.
- Finish [Godot web](engines/godot-web.md) acceptance on actual Windows browsers,
  physical laptop sleep, and full-duration TURN credential renewal. Bazzite
  Flatpak Chrome has passed threaded/single-threaded exports, forced relay,
  background/recovery checks, saves, conflicts, and Join friend. The adapted
  upstream Dodge the Creeps project has passed gameplay, saves, and guest isolation.
- Finish [Infuser Play](engines/godot-native.md) acceptance on physical Windows
  hardware and the installed browser-login handoff. Keep the native preview
  limited to single-player Godot on Windows/Linux, including Bazzite.
- Validate the [PeerJS adapter](peerjs.md) against an existing user's game when
  its source is available. The independent fixture has passed real PeerJS
  DataConnections, standalone play, forced TURN, renewal, and recovery. This
  does not prove an unavailable project's exact integration.
- Expand engine-server recipes after the Node path is solid. Native multiplayer
  remains a separate study. Bevy is paused.
- Review achievements before introducing persistent achievement IDs and behavior.

Implemented foundations include conditional JSON slots and retained JSON/binary checkpoints,
isolated guest play, on-demand managed servers, peer presence/joinability,
Join friend on cards and avatars, Infuser Play self-updates, readable game URLs,
metadata/art updates, and agent issue reporting.
Use their linked contracts rather than treating old design notes as future work.
