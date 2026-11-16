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
For managed Node games, enable the [source-packaging recipe](server-packaging.md).
Stage images manually only for other runtimes or the advanced image route.
Wake requests never perform
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

Follow [Infuser installation](clubhouse-operations.md) for the current Caddy,
Google OAuth, membership, and firewall setup. Use DNS for the main domain and a
wildcard beneath it. Published games run at the root of isolated game/build/guest
hostnames; no path rewriting or per-game DNS changes are needed. The production
Caddy template routes these hosts to the platform, which checks scoped launch
sessions. Website Google cookies must not be shared with game subdomains.

Keep certificate state persistent and DNS/OAuth secrets outside the checkout.
The platform pins each browser launch to its build. Missing assets return 404;
SPA fallback applies only to appropriate navigation requests.

The older `infuser render-caddy` command renders manually configured static sites
and server routes. It is not the installation path for the Library and publishing
platform; do not replace the platform Caddy template with its output.

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
the operator for staged-image games. Managed Node builds have bounded image
retention and remove unused images through the publishing API.

## Diagnosing game failures

Use [server diagnostics](server-diagnostics.md) for app-scoped build/runtime logs,
retained shutdown/crash evidence, and controller journal commands. Give the game's
agent its existing publishing credential, never the controller key or SSH access.

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
