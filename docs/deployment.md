# Automatic server updates

Audience: server operators.

Infuser can download and install published releases automatically. The updater
runs on your server; no external service needs SSH access or your Google credentials.

## Build and publication

`Build and release` tests and builds both Linux server architectures and both
Infuser Play platforms. It publishes a complete `build-<commit>` release with
same-commit binaries, installers, manifests and checksums. Existing updater URLs
and installer aliases are retained. No Mac job is on this critical path.

A running build finishes while new pushes replace only the pending run, so a
burst consumes one running build plus the newest pending candidate rather than
continually cancelling useful work. A tested ancestor of main may publish even
if main advances. Publication checks ancestry and the current latest release;
an older rerun cannot replace a newer release. Missing assets, failed tests,
unknown ancestry or upload failure leave the release unpublished. Published
assets are never replaced. A queued candidate can still fail and require a retry.

`Mac publisher release` follows successful main server workflows and builds the
exact upstream commit. Its independent queue publishes the two tested Mac
architectures as `macos-<commit>` with its own manifest and checksums. These are
marked pre-release to prevent them becoming the server/Play `latest` release;
this is channel separation, not a waiver of tests. Mac capacity delays affect
only Mac publisher availability. Use the releases list, not `latest/download`,
for Mac downloads. Manual dispatch on main can retry that channel independently.

Both tracks use standard GitHub-hosted runners and no paid service. Linux
server publication still waits for Windows/Linux Play packaging to preserve
compatibility with existing clients that share GitHub's single latest channel.
Publication does not prove host installation: the updater may defer for active
games or roll back a failed activation. Verify the host build separately.

## Release path

1. A host timer checks the latest published manifest approximately once a minute.
2. The updater downloads the host's binary, checks its digest and validates the
   installed configuration with the candidate executable.
3. It waits for games and publishing to become idle, then closes new admission
   briefly. The verified candidate remains cached while it waits.
4. It atomically switches `current`, restarts the controller, and checks `/healthz`
   for the exact new build ID. Game containers continue running; peer rooms end
   when the controller restarts, so their players must rejoin.
5. If activation fails, it restores the previous binary and verifies its health.
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

Install the maintenance-aware daemon before replacing an older stable updater:
the new updater requires `/v1/maintenance` and fails closed if it is unavailable.
An operator can explicitly allow interruption with
`sudo /usr/local/libexec/infuser-updater update /etc/infuser/update.toml --force`.
Automatic updates never use that override. A deferred update over 24 hours or a
failed release appears in [host monitoring](monitoring-operations.md).

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
