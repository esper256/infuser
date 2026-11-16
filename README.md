# Infuser

A private game library. Kids make games with their agents, publish them, and
press Play together. Rust runs the platform; Caddy handles HTTPS; Docker isolates
servers when a game needs one. SQLite runs inside the Rust daemon. Optional
PeerJS support adds one small Node-based PeerServer container.

**Working platform:** Google sign-in plus adult-managed invitations, an online
library, engine-aware web publishing, immutable releases, isolated game hosts,
cloud saves, fresh guest sessions, positive emoji reactions, author tags, fullscreen,
and a copyable agent prompt with a three-hour setup code. The agent stores its own
app-scoped publishing credentials. Admin-only previews keep test games
out of the kids' library. SQLite stores platform data independently of games.

**Server infrastructure:** one shared process per game, readiness checks, hard
RAM/CPU limits, aggregate resource admission, fixed port allocation, expiring
leases, idle shutdown, restart recovery, persistent world volumes, authenticated
HTTP/WebSocket routing, and game-bound admission tickets. An optional nftables
helper grants authenticated player IPs temporary access to native game ports.

**Current engine focus:** Three.js/Vite, plus experimental Godot web and
[native Godot delivery](docs/engines/godot-native.md). Bevy remains paused while
the SDK is changing. The happy path starts with a game project, not a Dockerfile.
**Infuser** is the website; **Infuser Play** is the native app. Infuser Play handles native downloads, updates and guest saves;
native multiplayer and managed Godot server packaging remain studies.
Experimental [WebRTC peer hosting](docs/peer-hosting.md) uses a shared coturn relay.
Plain Node/WebSocket servers have a capability-gated [managed packaging recipe](docs/engines/node-server.md).
Other custom server images currently need operator staging. Peer-hosted
multiplayer must have relay fallback; direct-only networking is unsupported.

**Publishing a game:** start with the [agent instructions](docs/agents.md).
Use a [prebuilt publisher](docs/publisher-install.md) on Mac/Linux or
[HTTP publishing](docs/publishing-http.md) with existing tools on Windows,
Mac or Linux. No Rust compiler, Docker installation or Infuser repository checkout
is required. Building Infuser and running this repository's platform test suite
are tasks for [Infuser contributors](docs/development.md), not game authors.
An existing Vite project needs its usual Node tooling on the game-making machine,
not on the Infuser host. Godot exports likewise use the creator's Godot toolchain.

- [Agent instructions and engine recipes](docs/agents.md)
- [Reporting platform problems](docs/agents.md#reporting-platform-problems)
- [Public publishing, player and cloud-save API](docs/public-api.md)
- [SDK 1.0 design review](docs/sdk-contract.md) — current game contract is experimental
- [Private feedback review setup](docs/feedback.md)
- [Player experience](docs/design.md) and [next steps](docs/roadmap.md)
- [Development checks](docs/development.md)
- [Install Infuser](docs/clubhouse-operations.md)
- [Private groups and operator access](docs/private-groups.md)
- [Controller operations](docs/operations.md) and [private lifecycle API](docs/api.md)
- [Automatic updates](docs/deployment.md) and [game-port firewall](docs/network-admission.md)
- [Product boundaries](docs/architecture.md)

Private domains, provider details and credentials live outside the repository.
Use ignored `.local/` files for installation configuration. Development does not
wait for CI. [Advisory checks and dependency maintenance](docs/ci.md) provide
feedback independently of the existing build-and-release workflow.

Licensed under the [MIT License](LICENSE).
