# Infuser

A private game library where creators' agents handle publishing and players just
press Play. Rust for the controller; Docker for game processes; Caddy for the web.

The first working slice is the infrastructure. The daemon starts a server when a
session requests it, waits for its Docker health check, and stops it after the
last player lease expires and the idle grace period passes. RAM and CPU are
reserved before launch and released only after confirmed removal. Container
limits are hard limits, and the total budget leaves room for the host.

**Implemented:** authenticated lifecycle API, idempotent launches, expiring
session leases, fixed per-app port allocation, resource admission, idle shutdown,
restart recovery, failure backoff, disk admission threshold, architecture checks,
persistent server volumes, Caddy route generation, explicit solo/multiplayer
manifest validation, a sample Rust game server, and automatic release updates
with health checks and rollback.
An optional game-port firewall provides default-deny access with expiring IP
grants; connecting grants to Google login is part of the upcoming player API.

**Next:** the Google-authenticated library UI and player API, agent publishing
API/CLI, immutable builds and branches, lobbies/presence, cloud saves, and engine
adapters. The current control API is a loopback-only operator/backend API, **not
the public publishing API**. Caddy's Google-login integration expects a separately
configured OAuth2 Proxy; the project does not yet ship a player website.

```sh
cargo build --locked --release --bin infuser
cargo test --locked
target/release/infuser check deploy/infuser.example.toml
```

Requires Rust 1.89+ to build. The resulting Linux executable needs Docker's CLI
and socket, but no Python, Node, database service, or Rust installation at runtime.
Build on ARM64 for an ARM64 host. Game images must match the host architecture.

Start here:

- [Run a server](docs/operations.md)
- [Automatic deployment](docs/deployment.md)
- [Lifecycle API](docs/api.md)
- [Game-port admission firewall](docs/network-admission.md)
- [Game integration instructions for agents](docs/agents.md)
- [Product boundaries and Steam-style concepts](docs/architecture.md)

Tests focus on journeys through the HTTP API, with a simulated container runtime
and an advanced clock so idle shutdown does not require real-time waits. An
opt-in Docker journey exercises the actual sample game. A small CI workflow
publishes passing builds; local development never waits for it.
