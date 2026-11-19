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
firewall policy for unrelated applications.

Use `deploy/Caddyfile.host.example` as the host-owned entry point. It imports
`/etc/caddy/sites-enabled/*.caddy`; put Infuser's existing site configuration in
`infuser.caddy` and keep Caddy's global options in the root file. Mount the whole
sites directory read-only into Caddy, so adding a file does not require a new
container. Keep certificate volumes and DNS credentials unchanged.

For another domain, point its DNS at the same host, add a new `.caddy` file using
`deploy/sites-available/project.caddy.example`, and choose an unused local
upstream port. Configure that project's authentication in its own site. Inside
the Caddy container, run `caddy validate --config /etc/caddy/Caddyfile`, then
`caddy reload --config /etc/caddy/Caddyfile`. Reload applies routing without
restarting other applications. Validate the combined config before every reload;
an invalid fragment must not replace the running configuration. No placeholder
domain should be enabled or request a certificate before its DNS is ready.

Grafana, Prometheus, node_exporter and blackbox_exporter can also be shared.
Separate project dashboards, scrape targets and alerts using folders and stable
project/service labels. These labels organize a trusted operator's monitoring;
they are not access isolation between unrelated tenants. Keep host-wide memory,
disk and monitoring-pipeline alerts under host ownership.

Infuser's alerts now select their own contacts directly and leave the shared
policy tree alone. Follow [monitoring setup](monitoring-operations.md) for new
scrape files, dashboard folders and the one-time migration from older policy
provisioning. Another project's failed target does not become an Infuser failure.

Infuser retains ownership of scheduled host reboots. Other projects inherit that
maintenance window and should stop cleanly; existing systemd shutdown inhibitors
are still honored. No additional project admission framework is required.
Resource allocation and each project's application backup plan can be settled
when that project is added; this setup does not expand Infuser's backup scope.

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
maintenance API for admission under the **strict host policy**. That policy
waits for open games, shared games, reserved game servers, publishing, and
server builds. It does not use the update's 10-minute or 2-hour caps, and
Update anyway does not apply. The admin update panel names what a deferred
reboot is waiting on. An unavailable controller or failed admission prevents
the reboot too. Shutdown inhibitors are honored; no forced reboot is used.
Admission remains closed during shutdown. Failed or check-only attempts
release the lease; an abandoned lease expires automatically.

A release can name a reboot or an Ansible run it cannot take effect without.
Put that in `release.json` as `"host": ["reboot"]`, `"host": ["ansible"]`, or
both. The field is omitted when the release needs neither. An updater installed
before this field existed rejects a manifest that includes it, so install this
updater before publishing a release that sets `host`. While that step is still
outstanding, the reboot or Ansible run uses the **release host policy**: the
same waits and Update anyway override as a binary update. The step is cleared
after the machine boots (for a reboot) or after Ansible finishes. `deploy/host-plan.json`
is what the release publisher copies into the manifest.

If the window stays busy, the next attempt is the following day. Inspect
`journalctl -u infuser-reboot.service` if a required reboot keeps being deferred;
this policy never escalates to kicking players off automatically.

The installer disables Ubuntu's separate automatic-reboot mechanism so it cannot
bypass these checks. Package installation remains enabled. Backup and reboot
commands use a common lock; the backup lock covers remote upload and verification
as well as local staging. The updater takes that same lock internally for
validation, activation and interrupted-update recovery, allowing downloads while
another job holds it. Do not wrap the new updater in the lock again. Use the
wrapper for other manually launched maintenance. Custom unit paths need matching
drop-ins. See [updater adoption](deployment.md#update-from-admin-tools).

To inspect without rebooting, run the same guarded operation in check mode:

```sh
sudo /usr/local/libexec/infuser-host-maintenance-lock \
  /usr/local/libexec/infuser-reboot-if-required --check
```

This still obeys the flag and window. Do not create a fake reboot-required flag
on a live machine to test it. The developer fixture uses disposable files and a
fake shutdown command.

## Shared service updates

For repeatable configuration installation and selected Compose updates, use
[Ansible host maintenance](ansible-operations.md). The manual procedure below
remains available for services not yet adopted into its private inventory.

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
