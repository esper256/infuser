# Infuser

A private library for family-friendly games. Users make games with their agents, publish
them, and press Play together. Google sign-in and admin-managed invitations keep
each private group's games, players, and chat together.

**Infuser** is the website. It provides cloud saves, fresh guest play, friends'
online status, reactions, tags, and game artwork. **Infuser Play** is the native
app for downloading, updating, and running supported Windows and Linux games,
including on Bazzite.

## Make or publish a game

To make a new game for an Infuser website, give its URL to your agent and start
with [Create a game for Infuser](docs/create-game.md). That guide explains the
features, supported engines, and multiplayer choices. No publishing token is
needed to design or build the game.

When the game is ready, sign in to your Infuser website, click **Add a game**, and
copy its prompt into the game's existing agent chat. The agent follows the
[game integration guide](docs/agents.md), keeps the project's name and tools,
and stores its publishing connection for later updates.

Use a [prebuilt publisher](docs/publisher-install.md) on Mac/Linux or
[HTTP publishing](docs/publishing-http.md) with existing tools on Windows,
Mac, or Linux. No Rust compiler, Docker installation, or Infuser checkout is
required on the game author's machine.

Three.js/Vite and plain browser games are supported. Godot web, native Godot,
and the game SDK are previews; see the [engine recipes](docs/agents.md#choose-the-integration)
for their limits. [Unity Web](docs/engines/unity-web.md) has experimental hosting
and an adapter awaiting a compiled Unity playtest. Plain Node/WebSocket backends can be packaged by Infuser.
Other server runtimes need an image prepared by the operator. Peer-hosted games
must support Infuser's relay fallback; direct-only networking is unsupported.

## Find your guide

- Players and admins: [chat and phone notifications](docs/library-chat.md),
  [Infuser Play updates](docs/play-updates.md), and [feedback](docs/feedback.md).
- Game authors and agents: [design a new game](docs/create-game.md), [integration and publishing](docs/agents.md),
  [art requirements](docs/card-art-guide.md), and [API reference](docs/public-api.md).
- Server operators: [install Infuser](docs/operations.md), [Google login and invitations](docs/clubhouse-operations.md),
  [private groups](docs/private-groups.md), and [automatic server updates](docs/deployment.md).
- [All documentation](docs/README.md), including the separate Infuser developer guides.

Licensed under the [MIT License](LICENSE).
