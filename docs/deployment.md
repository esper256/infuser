# Automatic server updates

Audience: server operators.

Infuser can download and install published releases automatically. The updater
runs on your server; no external service needs SSH access or your Google credentials.

## Release path

1. A host timer checks the latest published manifest approximately once a minute.
2. The updater downloads the host's binary, checks its digest and validates the
   installed configuration with the candidate executable.
3. It atomically switches `current`, restarts the controller, and checks `/healthz`
   for the exact new build ID. Game containers continue running; peer rooms end
   when the controller restarts, so their players must rejoin.
4. If activation fails, it restores the previous binary and verifies its health.
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
with `sudo install -m 0755 /opt/infuser/current/infuser /usr/local/libexec/infuser-updater`
after a version is known to work. The timer uses root only for installation and
systemd restart; the daemon continues running as the `infuser` service user.

Keep `/etc/infuser` and platform data outside release directories. The updater
retains the current and previous version; it does not overwrite configuration,
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
