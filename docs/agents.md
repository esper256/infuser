# Integrate a game with Infuser

Read this first. The current release hosts static web builds and manages server
containers. The public publisher API, CLI upload workflow, player website, cloud
saves and engine SDKs are planned, not available endpoints. For now, an operator
stages artifacts and adds the app to the private TOML catalog.

## Declare play modes

Use [the manifest example](../examples/game-manifest.json) and run
`infuser validate-game PATH`. `play_modes` can contain solo, multiplayer, or both.
Declare `solo_runtime` for solo support and a `multiplayer` configuration for
multiplayer support. That configuration names the architecture, transport,
capacity and join policy. Do not infer topology from the engine.

For `peer_hosted`, direct connections may be tried first, but
`relay_fallback: "required"` is mandatory. Prove the game works with direct
connections disabled. Raw peer TCP/UDP, LAN-only discovery and asking players to
forward ports are not supported. Dedicated servers may use their public endpoint.
Query installation capabilities before publishing: peer-hosted and relayed
runtime services are not implemented in this first slice, even though their
manifest contracts are defined.

## Browser client

1. Export a normal web build whose entry point is `index.html`. It will be served
   at `/` on its own hostname, so root-relative asset URLs work.
2. Place the whole export directory in an immutable release directory. Do not
   rename engine-generated files. Have the operator point a `current` symlink at
   the release and configure `[sites.APP_ID]`.
3. Set `spa=true` only for a client-side router. Missing JS/WASM/assets should stay
   404. Enable `cross_origin_isolation` only if the export requires shared memory
   or threads, and ensure external resources satisfy its cross-origin policy.
4. A solo web game does not need a game container. Local storage is not cloud
   storage; cloud-save API integration will be added separately.

## Server build

- Build a Linux image for the installation's architecture, or publish a
  multi-architecture image. Pin the manifest to its digest, never a mutable tag.
- Listen on `0.0.0.0:$INFUSER_PORT`. This is the internal port; the host port comes
  from the launch API and may differ.
- Run as a non-root numeric UID:GID, default `1000:1000`.
- Provide a Docker `HEALTHCHECK` that succeeds only when the server can accept
  players. Merely having a running PID is insufficient.
- The root filesystem is read-only. Use `/tmp` for temporary work (32 MiB,
  executable files prohibited) and `/data` for persistent world data when enabled.
  Make `/data` writable by the configured user in the image. Do not declare
  Dockerfile `VOLUME`; Infuser owns volume lifecycle.
- Handle SIGTERM: stop accepting joins, checkpoint and exit within `stop_seconds`.
  Docker kills a process that exceeds that deadline. Persistent volume contents
  survive sleep; memory does not.
- Test the requested RAM/CPU limits. A killed/OOM/unhealthy process is marked
  failed and cleaned up; the controller does not run an unlimited restart loop.
- Use `web=true` for HTTP/WebSocket servers. Caddy terminates TLS, so internal
  communication can use HTTP. The public URL is `APP_ID-server.DOMAIN`.

See [the Rust example](../src/bin/infuser-demo.rs) and its
[Dockerfile](../examples/demo/Dockerfile). Build it with:

```sh
docker build -f examples/demo/Dockerfile -t infuser-demo .
docker image inspect infuser-demo --format '{{.Id}}'
```

## Launch lifecycle

Each app has one shared multiplayer session per Infuser installation. The first
player starts it; later players join it. Do not create parallel rooms, servers or
peer hosts for the same app. A full or mid-round game should expose a waiting or
spectating state if supported, rather than create another session. Solo play on
individual devices is unaffected. This rule applies across release branches too.

The trusted launcher requests a lease using the [API](api.md). A game adapter
will receive the app ID, lease ID, scoped lease token and API address; it never
receives publishing or control credentials. Renew while starting and while
playing, wait for readiness, and use the returned endpoint. Release on exit when
possible; lease expiry handles crashes. A fixed timer is not a reliable way to
decide that a server is ready.

Future build-aware launchers must give new arrivals a client compatible with the
active session, even if a newer build has been published. Activate the new server
build only after the shared session ends or restarts. Peer-hosting integrations
will use Infuser's coordinated host assignment; an unsuccessful connection is
not permission to create a competing host.

Direct game sockets need their own admission check; Google login on the web page
alone does not secure a raw TCP/UDP port. Do not expose them publicly before the
game's admission integration is present. For integration tests, use loopback.

## Vocabulary for future publishing integrations

Keep an app's identity stable. Publish immutable builds, test on `preview`, and
promote to `default`. Separate web/native client artifacts from the server image.
Keep lobbies, sessions, cloud saves and presence distinct. Use explicit save-schema
versions and conditional cloud-save writes once that API is implemented.
