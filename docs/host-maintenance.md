# Host patching and service images

Audience: server operators.

## Sharing a host with other projects

Infuser does not require a dedicated VM. It can create game containers dynamically
while other projects use fixed, independently named Compose stacks on the same
Docker engine. Keep each project's networks, volumes, credentials and deployment
files separate. Reserve `org.infuser.*` labels for Infuser: its game inventory and
destructive operations use ownership labels, not a list of all Docker containers.
An exception list is not required to protect unrelated containers from game cleanup.
Avoid host-wide Docker pruning in any project's deployment scripts.

Keep a private host inventory listing each project, Compose project name, domains,
local ports, CPU/RAM limits, persistent data, backup owner and maintenance needs.
This is an operator record, not a new Infuser registry or container scheduler.
Infuser's game budget excludes other projects and shared services; lower it when
reserving capacity for another workload. Present idle memory is not a reservation.

Share one Caddy for ports 80/443, with a separate domain/site definition per
project. Other applications can bind loopback ports and receive traffic through
Caddy without inheriting Infuser's Google login or game authorization. Allocate
ports outside the configured Infuser game/relay range and existing service ports.
The Infuser ingress table protects its configured range; it is not a general
firewall policy for unrelated applications. Prefer a host-owned root Caddyfile
importing per-project site files before multiple projects start editing routing.

Grafana, Prometheus, node_exporter and blackbox_exporter can also be shared.
Separate project dashboards, scrape targets and alerts using folders and stable
project/service labels. These labels organize a trusted operator's monitoring;
they are not access isolation between unrelated tenants. Keep host-wide memory,
disk and monitoring-pipeline alerts under host ownership.

Before adding the second project, remove these current single-project assumptions:

- **Alert routing:** `render-notifications.sh` currently provisions the entire
  Grafana notification policy tree. Do not use it over another project's policies.
  Move policy-tree ownership to host configuration and make Infuser contribute
  only its own rules/contact points. Scope its broad scrape/website queries so
  another application's failure is not attributed to Infuser.
- **Reboot admission:** the current timer checks Infuser activity only. Other
  projects must be able to block shutdown with systemd shutdown inhibitors and
  coordinate maintenance through the shared host lock. Do not restart Docker or
  the host as though only Infuser is affected.
- **Recovery:** a whole-volume snapshot includes colocated data but remains
  crash-consistent. Infuser's application backup covers its own data, not another
  project's database. Give every project an explicit backup/restore plan.

No separate VM, Docker daemon or monitoring stack is necessary for this model.
All projects still share the host's capacity and the effects of kernel/Docker
maintenance; keep the setup sized and operated accordingly.

## Required reboots

Keep Ubuntu unattended security package updates enabled. Infuser can schedule
the resulting reboot without interrupting active games or its backups.
Install Bash, curl, jq and util-linux (`flock`), then run from the release sources:

```sh
sudo bash deploy/bootstrap-host-maintenance.sh America/Los_Angeles
```

Choose your installation's IANA time zone. It is stored in private host
configuration; the public timer defaults to UTC. The installer does not reboot.
For non-default API ports or token paths, edit `/etc/infuser/reboot.conf` first
using `deploy/reboot.example.conf` and the intended time zone.

The timer checks every five minutes between 04:00 and 04:25 in that zone,
including daylight-saving changes. It acts only when Ubuntu has created
`/var/run/reboot-required`. An empty window causes no reboot. Missed runs are not
replayed after boot, and the script refuses starts outside 04:00–04:30.

Before requesting an orderly reboot, it acquires the same host lock as full
backup and deployment jobs, checks package-update services, and asks Infuser's
maintenance API for idle admission. Active players, reserved game servers,
publishing work or another maintenance lease defer the reboot. An unavailable
controller or failed admission prevents it too. Shutdown inhibitors are honored;
no forced reboot is used. Admission remains closed during shutdown. Failed or
check-only attempts release the lease; an abandoned lease expires automatically.

If the window stays busy, the next attempt is the following day. Inspect
`journalctl -u infuser-reboot.service` if a required reboot keeps being deferred;
this policy never escalates to kicking players off automatically.

The installer disables Ubuntu's separate automatic-reboot mechanism so it cannot
bypass these checks. Package installation remains enabled. Systemd drop-ins wrap
the existing Infuser backup/update commands with a common lock; the backup lock
covers remote upload and verification as well as local staging. Use the wrapper
for manually launched maintenance too. Custom unit paths need matching drop-ins.

To inspect without rebooting, run the same guarded operation in check mode:

```sh
sudo /usr/local/libexec/infuser-host-maintenance-lock \
  /usr/local/libexec/infuser-reboot-if-required --check
```

This still obeys the flag and window. Do not create a fake reboot-required flag
on a live machine to test it. The developer fixture uses disposable files and a
fake shutdown command.

## Shared service updates

`deploy/services/compose.yaml` is the tracked image/build definition for Caddy,
oauth2-proxy and coturn. Merge it with private configuration supplying mounts,
networking, resource limits, credentials and persistent volumes. It is not a
complete stack to start on its own. Put the tracked file first so Caddy's build
context resolves correctly, and remove duplicate image/build entries from the
private file:

```sh
docker compose -p infuser -f deploy/services/compose.yaml \
  -f /absolute/private/services.yaml config --quiet
```

The Caddy image includes the Porkbun DNS provider. Its Go manifest and lockfile
track the actual server and provider code; its Dockerfile tracks the build and
runtime images. Do not mount an older host binary over `/usr/bin/caddy`, which
would bypass those updates. Other DNS providers require an adjusted custom build.
Certificate data stays in the existing persistent volumes.

Dependabot proposes weekly updates for service images, monitoring images, the
PeerServer base image and the Caddy Go dependencies. Digest pins select exact
images while version tags identify their update stream. This is update discovery
and testing, not a host service that blindly replaces running containers.

For an update:

1. Build the candidate Caddy image and run the service image checks. The full
   manual Docker/Caddy workflow tests authentication and WebSocket forwarding
   against that same tracked build. Repeat forced-relay and login acceptance when
   changing coturn or oauth2-proxy; a binary smoke check does not prove those flows.
2. Keep the previous image IDs and private configuration available for rollback.
   Check the resolved configuration privately; it can include credentials.
3. Apply only changed services during idle maintenance, under the host lock and
   controller admission lease. Preserve volume names, TLS state, secret mounts,
   listener ports, firewall rules and resource limits. Do not use `down -v`.
4. Verify website/login, controller health and the affected multiplayer path.
   Restore previous image references if acceptance fails.

An image PR or an Infuser binary release does not itself update installed host
configuration. Deploy the reviewed service definitions explicitly. Never let an
old private image override silently mask an updated tracked definition.

References: [Ubuntu automatic updates](https://documentation.ubuntu.com/security/security-updates/),
[systemd timers](https://www.freedesktop.org/software/systemd/man/latest/systemd.timer.html),
[Dependabot ecosystems](https://docs.github.com/en/code-security/reference/supply-chain-security/supported-ecosystems-and-repositories),
[Compose merge rules](https://docs.docker.com/compose/how-tos/multiple-compose-files/merge/).
