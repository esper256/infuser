# Host patching and service images

Audience: server operators.

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
