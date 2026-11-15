# Infuser

A private game library. Kids make games with their agents, publish them, and
press Play together. Rust runs the platform; Caddy handles HTTPS; Docker isolates
servers when a game needs one. No Python, Node, or database service on the host.

**Working platform:** Google sign-in plus adult-managed invitations, an online
library, engine-aware web publishing, immutable releases, isolated game hosts,
cloud saves, fresh guest sessions, positive emoji reactions, author tags, fullscreen,
and a copyable agent prompt with a one-hour setup code. The agent stores its own
app-scoped publishing credentials. Admin-only previews keep test games
out of the kids' library. SQLite stores platform data independently of games.

**Server infrastructure:** one shared process per game, readiness checks, hard
RAM/CPU limits, aggregate resource admission, fixed port allocation, expiring
leases, idle shutdown, restart recovery, persistent world volumes, authenticated
HTTP/WebSocket routing, and game-bound admission tickets. An optional nftables
helper grants authenticated player IPs temporary access to native game ports.

**Current engine focus:** make the full Three.js/Vite journey solid before expanding
compatibility. Godot and Bevy work is paused while the SDK is changing. The happy
path starts with a game project, not a Dockerfile. Server
engine packaging, native client distribution, and peer relay/signaling are still
in progress. Custom server images currently need operator staging. Peer-hosted
multiplayer must have relay fallback; direct-only networking is unsupported.

```sh
cargo build --locked --release --bin infuser
cargo test --locked
infuser publish /private/game.credentials.json /path/to/vite-game --solo
```

Requires Rust 1.89+ to build. Releases provide Linux AMD64/ARM64 executables.
An existing Vite project needs its usual Node tooling on the game-making machine,
not on the Infuser host. Godot exports likewise use the creator's Godot toolchain.

- [Agent instructions and engine recipes](docs/agents.md)
- [Public publishing, player and cloud-save API](docs/public-api.md)
- [SDK 1.0 design review](docs/sdk-contract.md) — current game contract is experimental
- [Player experience](docs/design.md) and [next steps](docs/roadmap.md)
- [Install the clubhouse](docs/clubhouse-operations.md)
- [Controller operations](docs/operations.md) and [private lifecycle API](docs/api.md)
- [Automatic updates](docs/deployment.md) and [game-port firewall](docs/network-admission.md)
- [Product boundaries](docs/architecture.md)

Private domains, provider details and credentials live outside the repository.
Use ignored `.local/` files for installation configuration. Development does not
wait for CI; a small workflow only promotes tested builds to the live server.
