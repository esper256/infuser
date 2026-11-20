# Roll back a bad update

Audience: server operators.

Before the updater switches to a new release, it takes a consistent snapshot of
the platform SQLite databases and `state.json`. The snapshot is tied to the
release that is about to be replaced. If the new release does not become
healthy, the updater restores that snapshot and the `previous` release together.
You can also restore it yourself after an update that started but should not
stay.

This is the escape hatch for a bad controller release. It does not use restic,
Google Drive, or Oracle boot-volume backups. The separate
[full backup](backup-operations.md) remains the copy of world volumes and
configuration, and it stays disabled until you configure a repository.

## What is saved

- `platform.sqlite`, using SQLite's online backup API so the live WAL is
  included in one consistent file
- `publish-captures/requests.sqlite`, when that file exists
- `state.json`

Each database is integrity-checked, then copied again and checked a second time
before the snapshot is kept. One outgoing release is retained, under
`/var/lib/infuser-updater/rollback/<release>/`. The previous copy is removed
only after the new one is in place and verified. A partial or interrupted
snapshot, including one left by a reboot, is deleted on the next attempt.
Docker volumes, published builds, and the rest of the state directory are not
in this snapshot. A migration that rewrites world data still needs its own backup.

The updater refuses the snapshot when two copies of the databases would leave
less than 1 GiB free, or less than 5 percent of that filesystem, whichever is
larger. The running release stays in place, and host health reports
`rollback-snapshot`. `failed-release` is one small file that is replaced, not
appended.

The copy runs while the controller is still up, after activation is allowed and
the host maintenance lock is held. It does not pause games by itself. The
activation that follows still restarts the controller. A game already open in
the browser keeps running. A shared peer game ends and has to be started again.
A dedicated game server keeps running, but players' connections drop and can
rejoin. An agent's publish that is still arriving, or a server image that is
still being built, is lost.

## Enable it

The running updater is the separate binary `/usr/local/libexec/infuser-updater`,
not the daemon that automatic updates replace. Adopt a release that contains
this command, then install that binary with the
[helper procedure](deployment.md#separately-installed-helpers). Install the
matching `deploy/infuser-update.service` as well and reload systemd. The unit
must be allowed to write `/var/lib/infuser`: a WAL snapshot updates SQLite's
index, and an automatic restore replaces the database files. If the platform
`state_dir` is not `/var/lib/infuser`, add that directory to the unit's
`ReadWritePaths`.

No `backup.toml`, restic password, or Drive token is required. Leave the hourly
backup timer disabled until you want the full backup.

`/etc/infuser/update.toml` is the existing updater config. After the helper is
installed, snapshot the release that is running now so a later failed update has
something to restore:

```sh
sudo /usr/local/libexec/infuser-updater snapshot /etc/infuser/update.toml
```

That command does not stop Infuser. Every later activation refreshes the
snapshot of the outgoing release before it changes `/opt/infuser/current`.
Confirm the directory:

```sh
sudo /usr/local/libexec/infuser-updater rollback-verify /var/lib/infuser-updater/rollback/RELEASE
```

`RELEASE` is the directory name `readlink -f /opt/infuser/current` ends with, or
`bootstrap` for the initial install. Host health reports a failed snapshot
attempt through the existing updater state directory. No snapshot yet is not a
failure. A failed attempt is the `rollback-snapshot` check in
[host monitoring](monitoring-operations.md).

## One-command restore

This stops Infuser, restores the snapshot that matches `previous`, points
`current` at that release, and starts Infuser again. Players are disconnected.
The release you left is written to `/var/lib/infuser-updater/failed-release`, so
the update timer does not install it again. Delete that file only when you
intend to retry it.

```sh
sudo /usr/local/libexec/infuser-updater rollback /etc/infuser/update.toml
```

The command refuses to start when another update holds the lock, when
`current` is already the previous release, or when that release has no verified
snapshot. It does not roll back a second older release. On an activation that
fails its own health check, the updater runs this restore by itself before it
starts the previous binary. If the snapshot cannot be restored, it leaves the
new selection in place rather than starting an older binary against migrated
state.

## Restore by hand

Stop the update timer first so it cannot change `current` while you copy files.
Use the release id that `/opt/infuser/previous` points at. Do not copy `-wal` or
`-shm` files; with the service stopped, delete the live sidecars so SQLite
cannot replay them over the restored database.

```sh
set -eu
sudo systemctl stop infuser-update.timer
sudo systemctl stop infuser.service
release=$(basename "$(readlink -f /opt/infuser/previous)")
abandoned=$(basename "$(readlink -f /opt/infuser/current)")
snap=/var/lib/infuser-updater/rollback/$release
sudo /usr/local/libexec/infuser-updater rollback-verify "$snap"
install_file() {
  src=$1
  dest=$2
  if [ ! -f "$src" ]; then
    return 0
  fi
  owner=$(stat -c '%u:%g' "$dest")
  mode=$(stat -c '%a' "$dest")
  sudo rm -f "$dest-wal" "$dest-shm"
  sudo install -o "${owner%:*}" -g "${owner#*:}" -m "$mode" "$src" "$dest"
  sudo rm -f "$dest-wal" "$dest-shm"
}
install_file "$snap/platform.sqlite" /var/lib/infuser/platform.sqlite
install_file "$snap/publish-captures/requests.sqlite" /var/lib/infuser/publish-captures/requests.sqlite
install_file "$snap/state.json" /var/lib/infuser/state.json
sudo ln -sfn "/opt/infuser/releases/$release" /opt/infuser/current
printf '%s' "$abandoned" | sudo tee /var/lib/infuser-updater/failed-release >/dev/null
sudo systemctl start infuser.service
```

Check `http://127.0.0.1:7777/healthz` and confirm `build` is the restored
release. Start `infuser-update.timer` again after that. Adjust the state
directory if the host does not use `/var/lib/infuser`. Keep the service user
from `deploy/infuser.service` as the owner of every restored file; the command
above copies the owner and mode already on the destination.

## Games deleted after the snapshot

An admin can delete a hidden game after this snapshot is taken. The snapshot is a
copy of the databases and `state.json` from before that deletion, so restoring it
brings those catalog rows, saves, and publish-capture records back. Published
files, Docker volumes, containers, and images are not in the snapshot. A restored
catalog can therefore point at game files that were removed. Take a new snapshot
after a deletion you want to keep. This does not keep the deleted game's files.

References: [SQLite backup API](https://sqlite.org/backup.html),
[automatic updates](deployment.md), [encrypted backups](backup-operations.md).
