# Documentation

Choose the guide for what you want to do. Game integration guides describe using
Infuser; developer guides describe changing Infuser itself.

## Players and admins

- [Chat, phone installation, and notifications](library-chat.md)
- [Infuser Play updates](play-updates.md)
- [Send and review feedback](feedback.md)

## Game authors and their agents

Start with [Put an existing game on Infuser](agents.md). Use a [publisher download](publisher-install.md)
or [HTTP publishing](publishing-http.md); no Infuser build toolchain is required.

- Engines: [Three.js/Vite and HTML](engines/threejs-vite.md), [Node servers](engines/node-server.md),
  [Godot web](engines/godot-web.md), [Godot multiplayer](engines/godot-multiplayer.md),
  [Godot native](engines/godot-native.md), [Bevy limitations](engines/bevy.md).
- SDK: [versions and capabilities](sdk-versioning.md), [cloud saves](cloud-save-design.md),
  [on-demand servers](runtime-sessions.md), [dedicated sessions](dedicated-sessions.md),
  [WebRTC peers](peer-hosting.md), [PeerJS](peerjs.md), [presence](presence.md),
  [Join Friend](launch-intents.md).
- Publishing: [API reference](public-api.md), [Node packaging API](server-packaging.md),
  [server diagnostics](server-diagnostics.md), [game URLs](game-urls.md).
- Presentation: [cover art](card-art-guide.md), [metadata and art requests](game-metadata.md),
  [art-agent API](art-agent.md).
- Examples: [Node/WebSocket](../examples/dedicated-node/README.md),
  [Godot web](../examples/godot-web/README.md), [adapting an existing Godot project](../examples/godot-existing/README.md).
- [Report a platform problem](agents.md#reporting-platform-problems).

## Server operators

- [Install and run Infuser](operations.md), then [Google login and invitations](clubhouse-operations.md)
- [Automatic updates](deployment.md) and [game-port firewall](network-admission.md)
- [Encrypted backups and recovery](backup-operations.md), [host monitoring and alerts](monitoring-operations.md)
- [TURN and PeerJS services](relay-operations.md), [managed Node hosting](server-packaging-operations.md)
- [Private groups](private-groups.md), [feedback posting](feedback-operations.md), [art-agent keys](art-agent-operations.md)
- [Publication capture](publication-capture.md) and [private lifecycle API](api.md)

## Infuser developers

These pages are for changing the platform, not publishing a game.

- [Development setup and checks](development.md), [feature tests](developer-testing.md), [CI and releases](ci.md)
- [Architecture](architecture.md), [implementation and compatibility notes](implementation-notes.md), [roadmap](roadmap.md)
- [Architectural review improvement plan](proposals/architecture-improvement-plan.md)
- [SDK design review](sdk-contract.md), [accepted decisions](sdk-1.0-decisions.md),
  [1.0 stabilization plan](proposals/sdk-1.0-stabilization.md),
  [temporary visits](proposals/temporary-visits.md),
  [dedicated-server proposal](proposals/sdk-1.0-dedicated-servers.md), [implementation plan](proposals/dedicated-sdk-implementation.md)
- [UI design](design.md), [themes](themes.md), [branding assets](../assets/branding/README.md),
  [desktop UI](../desktop/README.md), [merge workflow](auto-merge.md)
