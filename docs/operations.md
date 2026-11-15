# Run Infuser

Use a Linux host with Docker Engine, the Docker CLI, and working cgroup memory,
swap, CPU and PID limits. The controller checks those capabilities on startup.
Keep the host's resources above the configured game budget; startup requires at
least 1 GiB of RAM outside it. Build on the target architecture:

```sh
cargo build --locked --release --bin infuser
sudo install -m 0755 target/release/infuser /usr/local/bin/infuser
sudo useradd --system --user-group --home-dir /var/lib/infuser infuser
sudo usermod -aG docker infuser
sudo install -d -o infuser -g infuser -m 0700 /var/lib/infuser
sudo install -d -o root -g infuser -m 0750 /etc/infuser
sudo install -o root -g infuser -m 0640 deploy/infuser.example.toml /etc/infuser/infuser.toml
infuser token | sudo tee /etc/infuser/control.secret >/dev/null
sudo chown root:infuser /etc/infuser/control.secret
sudo chmod 0640 /etc/infuser/control.secret
```

Edit the private config: domain, advertised host, budgets and game/site entries.
Stage game images with `docker pull` or a local build. Wake requests never perform
a network image pull or compile a game. This keeps launches predictable.

```sh
sudo -u infuser infuser init /etc/infuser/infuser.toml
sudo install -m 0644 deploy/infuser.service /etc/systemd/system/infuser.service
sudo systemctl daemon-reload
sudo systemctl enable --now infuser
```

The controller holds an exclusive state-directory lock. Its JSON state is replaced
atomically and synced to disk; it contains lease credentials and must stay private.
Back up that directory together with named game data volumes. Restore matching
state before restarting; unknown managed containers cause startup to fail rather
than disappear from resource accounting. Restarting the controller leaves games
running. Do not run two controllers against one Docker daemon.

Port allocation is stable for installed apps, including sleeping apps. To change
or remove a server manifest, stop that app through the API, wait for cleanup,
restart the controller with the edited catalog, then regenerate/reload Caddy.
Game removal never deletes its persistent volume.

## Web hosting and Google login

Set a DNS record for the main domain and a wildcard under it. Porkbun supports
wildcard A/CNAME records. For a deployment at `games.example.com`, the `example.com`
zone would use Host fields `games` and `*.games`, pointing to the server. This only
applies if that provider hosts the authoritative DNS zone.

Use Caddy 2.10+ on the host. Each static client gets `APP_ID.DOMAIN`; each web
backend gets `APP_ID-server.DOMAIN`. No asset-path rewriting is needed. TLS and
WebSocket upgrades are handled by Caddy. Server HTTP ports bind only to loopback.

```sh
sudo -u infuser infuser render-caddy /etc/infuser/infuser.toml > /tmp/Infuser.Caddyfile
caddy validate --config /tmp/Infuser.Caddyfile --adapter caddyfile
sudo install -m 0644 /tmp/Infuser.Caddyfile /etc/caddy/Caddyfile
sudo systemctl reload caddy
```

By default, Caddy obtains individual certificates, requiring ports 80/443 to reach
the server. Wildcard DNS does **not** require a wildcard certificate. For wildcard
certificates, render with `--wildcard` and use a Caddy build containing
`github.com/caddy-dns/porkbun`. Set `PORKBUN_API_KEY` and
`PORKBUN_API_SECRET_KEY` privately in Caddy's environment so DNS-01 renewal works.
The API key needs the `read` and `dns` permissions and API access enabled for the
domain. Restrict the key to the deployment's DNS zone; source-IP restrictions
must include the server's outbound public IP. Store secrets outside the checkout
on the server, for example in a root-owned `/etc/infuser/porkbun.env` with mode
`0600`, loaded by a systemd `EnvironmentFile` in the Caddy service. Keep Caddy's
data directory persistent so certificate state survives application updates.
The main domain still needs its own certificate. Unknown game hostnames return
404; the wildcard is not an arbitrary-container router.

Run OAuth2 Proxy separately at `127.0.0.1:4180`, starting with
[`oauth2-proxy.example.cfg`](../deploy/oauth2-proxy.example.cfg). Configure a Google
OAuth web client with `https://DOMAIN/oauth2/callback`, a private list of invited
email addresses (one per line), and the OAuth client/cookie secrets. Generate the
cookie secret with `openssl rand -base64 32` (it is not an Infuser control token). Set the cookie
domain and redirect allowlist to this installation. Verify supervised Google
accounts during setup. This is deployment configuration, not a completed website.

Visit `https://DOMAIN/oauth2/start` to log in. The initial Caddy configuration
returns 401 for an unauthenticated game request; the future launcher will provide
the Sign in with Google button. Auth-service failures deny access. Static files
and web backends are both gated. Do not set an unrestricted email allowlist.

Caddy must be able to read static release directories. Keep releases immutable and
switch the `current` symlink atomically; current responses use `no-cache` to reduce
stale entry points after updates. This does not yet pin assets for existing browser
sessions. Future build-specific URLs will provide that and immutable caching.
SPA fallback applies only to extensionless HTML
navigation, leaving missing WASM/JS/assets as 404.

## Network and storage

Expose 80/443 for the web. Only expose the configured game range for protocols
actually used by native games with admission authentication. Raw game sockets do
not inherit Google authentication. Keep port 7777 and the Docker socket private.
Docker installs its own firewall rules: check the host's Docker-aware filtering
and upstream firewall, not only an uncomplicated-firewall rule list.

Each server gets a separate Docker network, non-root UID, dropped capabilities,
read-only root, bounded temporary space/logs, and hard CPU/RAM/PID limits. These
limits keep accidents contained; agents do not get permission to choose arbitrary
host mounts, capabilities or Docker flags.

The free-disk threshold blocks **new starts**. It is not a disk quota: image caches
and persistent saves can still grow while games run. Monitor disk space and back
up worlds. Do not blindly prune game volumes. Images are staged and cleaned up by
the operator until the publisher manages build retention.

## Verification

`cargo test --locked` runs the fast API lifecycle journeys. To exercise Docker too:

```sh
docker build -f examples/demo/Dockerfile -t infuser-demo .
INFUSER_TEST_IMAGE=$(docker image inspect infuser-demo --format '{{.Id}}') \
  cargo test --locked --test docker_journey -- --ignored --nocapture
```

The Docker journey uses temporary state and loopback-only game ports. Run it as a
user with access to the Docker socket. It does not use a production catalog.

The optional Caddy journey runs the generated routing on temporary local HTTP
ports with a test authentication service and backend:

```sh
CADDY_BIN=/path/to/caddy cargo test --locked --test web_journey -- --ignored
```

It verifies the auth gate, SPA navigation, missing assets, WASM MIME type,
cross-origin headers and backend routing. It does not contact Google or issue
real TLS certificates.

References: [Docker limits](https://docs.docker.com/engine/containers/resource_constraints/),
[Docker port publishing](https://docs.docker.com/engine/network/port-publishing/),
[Porkbun wildcards](https://kb.porkbun.com/article/94-what-is-a-wildcard-dns-record),
[Caddy wildcard TLS](https://caddyserver.com/docs/caddyfile/patterns#wildcard-certificates),
[Google OAuth provider](https://oauth2-proxy.github.io/oauth2-proxy/configuration/providers/google/).
