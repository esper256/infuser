# Run Infuser

Audience: server operators.

Use a Linux host with Docker Engine, the Docker CLI, and working cgroup memory,
swap, CPU and PID limits. The controller checks those capabilities on startup.
Keep the host's resources above the configured game budget; startup requires at
least 1 GiB of RAM outside it.

Download `infuser-linux-amd64` for x86-64 or `infuser-linux-arm64` for aarch64 from
[one Infuser release](https://github.com/esper256/infuser/releases), along with its
`SHA256SUMS`. Verify the file with `sha256sum` against that same release. Save the
verified binary outside the extracted source archive. Download the source archive
from that tag for its
`deploy/` templates; no Rust compiler is needed. From the extracted archive:

```sh
sudo install -m 0755 /path/to/verified/infuser /usr/local/bin/infuser
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
For managed Node games, enable the [Node hosting setup](server-packaging-operations.md).
Stage images manually only for other runtimes or the advanced image route.
Wake requests never perform
a network image pull or compile a game. This keeps launches predictable.

```sh
sudo -u infuser infuser init /etc/infuser/infuser.toml
sudo install -m 0644 deploy/infuser.service /etc/systemd/system/infuser.service
sudo systemctl daemon-reload
sudo systemctl enable --now infuser
```

The state directory contains credentials and must stay private.
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

Game agents can use their publishing connection to read [server diagnostics](server-diagnostics.md). They do not need the controller key or SSH access.

`journalctl -u infuser --since "15 minutes ago"` includes app/attempt lifecycle
transitions and packaging job IDs. Runtime reports live under
`STATE_DIR/runtime-diagnostics/APP.json`, with private file permissions. A capture
failure is logged but does not prevent container cleanup or hold CPU/RAM forever.
Use the [private publication archive](publication-capture.md) for importer problems;
it records publishing inputs, not game WebSocket traffic.
For proxy errors, inspect the Caddy container's error log as well
(`docker logs --since 15m CADDY_CONTAINER`). The publisher diagnostics API does not
collect Caddy or browser logs.

## Check the installation

For OS patch reboots and shared-service image updates, follow
[host maintenance](host-maintenance.md). This is separate from Infuser's binary
updater and preserves the same idle-maintenance admission rules.

Run `infuser check /etc/infuser/infuser.toml`, then check
`systemctl status infuser` and `curl --fail http://127.0.0.1:7777/healthz`.
The health endpoint confirms the daemon is running. Test a game separately.

References: [Docker limits](https://docs.docker.com/engine/containers/resource_constraints/),
[Docker port publishing](https://docs.docker.com/engine/network/port-publishing/),
[Porkbun wildcards](https://kb.porkbun.com/article/94-what-is-a-wildcard-dns-record),
[Caddy wildcard TLS](https://caddyserver.com/docs/caddyfile/patterns#wildcard-certificates),
[Google OAuth provider](https://oauth2-proxy.github.io/oauth2-proxy/configuration/providers/google/).
