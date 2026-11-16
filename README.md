# Infuser

A private game library for families. Kids make games with their agents, publish
them, and press Play together. Google sign-in and adult-managed invitations keep
each private group's games, players, and chat together.

**Infuser** is the website. It provides cloud saves, fresh guest play, friends'
online status, reactions, tags, and game artwork. **Infuser Play** is the native
app for downloading, updating, and running supported Windows and Linux games,
including on Bazzite.

## Publish a game

Copy **Add a game** into the game's existing agent chat. The agent follows the
[game integration guide](docs/agents.md), keeps the project's name and tools,
and stores its publishing connection for later updates.

Use a [prebuilt publisher](docs/publisher-install.md) on Mac/Linux or
[HTTP publishing](docs/publishing-http.md) with existing tools on Windows,
Mac, or Linux. No Rust compiler, Docker installation, or Infuser checkout is
required on the game author's machine.

Three.js/Vite and plain browser games are supported. Godot web, native Godot,
and the game SDK are previews; see the [engine recipes](docs/agents.md#choose-the-integration)
for their limits. Plain Node/WebSocket backends can be packaged by Infuser.
Other server runtimes need an image prepared by the operator. Peer-hosted games
must support Infuser's relay fallback; direct-only networking is unsupported.

## Find your guide

- Players and adults: [chat and phone notifications](docs/library-chat.md),
  [Infuser Play updates](docs/play-updates.md), and [feedback](docs/feedback.md).
- Game authors and agents: [integration and publishing](docs/agents.md),
  [art requirements](docs/card-art-guide.md), and [API reference](docs/public-api.md).
- Server operators: [install Infuser](docs/operations.md), [Google login and invitations](docs/clubhouse-operations.md),
  [private groups](docs/private-groups.md), and [automatic server updates](docs/deployment.md).
- [All documentation](docs/README.md), including the separate Infuser developer guides.

Licensed under the [MIT License](LICENSE).
