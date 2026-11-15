# Fast development, automatic deployment

Development does not wait for CI. Push changes normally; one small workflow runs
the journey tests and builds Linux ARM64/AMD64 controllers, macOS ARM64/AMD64
publishers, and Windows/Linux Infuser Play installers.
Passing runs publish an immutable `build-COMMIT` GitHub release. A failed run
publishes nothing, so the live server keeps its working version while development
continues. New pushes cancel obsolete runs. There are no added PR approval gates,
release meetings, or manually assigned version numbers.

The server pulls releases instead of receiving SSH connections from CI. Its IP,
provider, SSH keys, Google credentials and admin identity are not needed in GitHub
Actions. The public repository contains only generic deployment templates.

## Release path

1. Tests and builds pass across the release matrix.
2. CI uploads the artifacts and SHA-256 manifest to a draft release.
3. It checks that the commit is still the main-branch head before publishing.
4. A host timer checks the latest published manifest approximately once a minute.
5. The updater downloads the host's binary, checks its digest and validates the
   installed configuration with the candidate executable.
6. It atomically switches `current`, restarts the controller, and checks `/healthz`
   for the exact new build ID. Game containers continue running; peer rooms end
   when the controller restarts, so their players must rejoin.
7. If activation fails, it restores the previous binary and verifies its health.
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
Only sanitized examples belong in `deploy/`. Do not force-add private files.

This workflow releases Infuser itself. Game agents publish immutable builds
independently through the publishing API. Hidden previews are a visibility
setting, not separate branches. Game updates do not require a daemon rebuild.
