# Automatic server updates

Audience: server operators.

Infuser can download and install published releases automatically. The updater
runs on your server; no external service needs SSH access or your Google credentials.

## What is updated

The server and Windows/Linux Infuser Play assets are published together after
checks pass. Mac publisher downloads have a separate release channel; a delayed
Mac build does not hold up server updates. Failed builds leave the current
published release available. See [CI and release builds](ci.md#release-builds)
for contributor workflow details.

This updater installs the controller executable. It does not update shared
service images, systemd units, Caddy routing, monitoring files or the separate
backup/recovery executable. Follow [host maintenance](host-maintenance.md) for
service changes and [backup operations](backup-operations.md#enable-and-verify)
for the backup worker. Keep these changes explicit when adopting a release.

## Release path

1. A host timer checks the latest published manifest approximately once a minute.
2. The updater downloads the host's binary and checks its digest, even while games,
   publishing or another maintenance job prevent activation. Under the shared host
   lock, it validates configuration and identity-state compatibility.
3. It waits for games and publishing to become idle, then closes new admission
   briefly. The verified candidate remains cached while it waits.
   Every pass looks for a newer release. It checks GitHub again after staging;
   if another release appeared, it leaves the older candidate inactive and fetches
   the newer one on the next pass.
4. It atomically switches `current`, restarts the controller, and checks `/healthz`
   for the exact new build ID. Game containers continue running; peer rooms end
   when the controller restarts, so their players must rejoin.
5. If activation fails, it checks the previous binary can read current state,
   restores it and verifies its health. An incompatible rollback is blocked.
   The failed release is held until a newer build arrives or an operator clears
   the local `failed-release` file to retry it.

The manifest and binaries are downloaded through GitHub release URLs, avoiding
continuous unauthenticated API polling. Checksums detect corruption; trust comes
from HTTPS and control of the repository/release workflow, not from the checksum
alone. Protect that repository account as you would code access to the host.

## Enable it

First install and start a working controller using [operations.md](operations.md).
Create `/etc/infuser/update.toml` from
[`update.example.toml`](../deploy/update.example.toml), setting the public source
repository. Then:

```sh
sudo bash deploy/bootstrap-updater.sh /usr/local/bin/infuser
sudo systemctl start infuser-update.service
journalctl -u infuser-update.service -n 30
```

The bootstrap preserves the current working binary under `/opt/infuser/releases`.
It installs the updater as a separate stable executable so it can recover even
when a candidate daemon will not run. Updater changes can be installed explicitly
by staging the new executable beside `/usr/local/libexec/infuser-updater` and
atomically renaming it into place through the host maintenance procedure
after a version is known to work. The timer uses root only for installation and
systemd restart; the daemon continues running as the `infuser` service user.

**First identity-genesis deployment:** upgrade the stable updater before activating
the migration. Follow the [identity migration procedure](identity-operations.md),
including the on-host rehearsal and retained snapshot. Genesis is the oldest
supported rollback target after migration.

Install the maintenance-aware daemon before replacing an older stable updater:
the new updater requires `/v1/maintenance` and fails closed if it is unavailable.
An operator can explicitly allow interruption with
`sudo /usr/local/libexec/infuser-updater update /etc/infuser/update.toml --force`.
Unattended updates do not interrupt activity without an operator's one-time
authorization. A deferred update over 24 hours or a
failed release appears in [host monitoring](monitoring-operations.md).

## Update from Admin tools

The server operator sees **Update available** inside **Admin tools** when a
published release is waiting. It shows elapsed waiting time and the current
reason: games, publishing, other requests or another maintenance job. A newer
release does not reset the waiting counter. Regular private-group admins do not
get this server-wide control.

Choose **Update anyway**, then **Authorize one update** to permit one activation
that can disconnect games and interrupt uploads or publishing across all groups.
The updater picks it up on its next pass, normally within about a minute. It
rechecks GitHub and uses the latest published, verified candidate at that check.
An authorization lasts ten minutes, survives a controller restart, and is consumed
before activation. It cannot be replayed or carried into a different running build.
It does not bypass another maintenance job, checksums, configuration/state checks,
failed-release quarantine or rollback checks. No software can guarantee that a
new release will not appear immediately after the final GitHub lookup.

For an existing installation, adopt the new stable updater **and** the tracked
`infuser-update.service` and `maintenance-lock-update.conf` together using
[Ansible](ansible-operations.md). The old wrapper took the host lock before the
download; the updater now takes it internally before validation, activation or
interrupted-update recovery. Do not remove the wrapper while leaving an old
updater installed. The daemon must be updated too before the web controls work.

Status and authorizations pass over authenticated loopback control routes; the
website cannot run commands or change the release source. The daemon stores the
small durable record in `deployment.json` in its state directory. Keep this file
private with the rest of that directory. There are no additional credentials or
public management ports to configure.

Keep `/etc/infuser` and platform data outside release directories. The updater
retains current, previous and the latest candidate while waiting; it does not overwrite configuration,
credentials, worlds or saves. Automatic binary rollback requires state formats
to remain backward-compatible. A release with an irreversible data migration
must use a separately planned maintenance path.

To pause automatic deployment, stop the timer:

```sh
sudo systemctl disable --now infuser-update.timer
```

## Local-only settings

`.local/` is ignored by Git and excluded from Docker builds. Use it for private
deployment configuration, OAuth credentials, publisher connection files and
local state. Files named `*.credentials.json` and `*.secret` are ignored too.
Keep all real credentials and private deployment settings out of Git.

This updates Infuser itself. Games are published separately through the publishing
API and do not require a server software update.
