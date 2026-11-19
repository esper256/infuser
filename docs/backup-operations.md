# Encrypted backups and recovery

Audience: server operators.

A bad controller release has a separate, smaller escape hatch: before each
update, Infuser snapshots platform SQLite and can restore it with the previous
binary. That path does not use this repository. See
[Roll back a bad update](update-rollback.md).

Infuser uses restic for encrypted backup storage and rclone for Google Drive
transport. Use a **dedicated repository**. Keep the repository address, encryption
password and limited Drive credentials in a separate recovery location as well
as on the host. Losing the only password copy makes the backup unusable.

Only the latest restore-verified snapshot is retained. Its predecessor stays
until the replacement uploads, restores into a fresh local directory, and passes
file, ownership, permission and database checks. Failures retain the previous
healthy copy. Replacement temporarily needs space for both; this is not a
historical archive. Failed uploads can leave unused repository data until a
later successful prune.

## Configure storage

Install restic and, for Drive, rclone from their official packages. Keep real
settings under `/etc/infuser`, outside the checkout. Copy
`deploy/backup.example.toml` to `/etc/infuser/backup.toml` and adjust paths. The
worker runs as root to preserve file ownership and read managed Docker volumes.

For Google Drive:

1. Create a separate Google OAuth client for backup use. Enable the Drive API.
   Do not reuse the website login client or a token previously granted full
   Drive access.
2. Configure an rclone Drive remote named `infuser-backup` using that client,
   selecting **`drive.file`** scope. Authorize it in your own browser. This allows
   access to files the app creates or that you explicitly grant it; it does not
   grant general access to your Drive. Unattended access still requires a limited
   refresh token on the host. Protect its config file with mode `0600`.
3. Set `use_trash = false` in that remote's private rclone configuration. Restic
   removes superseded backup objects; sending those to Drive trash would retain
   their quota usage. Let this remote create its own new backup folder.
4. Put `rclone:infuser-backup:infuser-restic` in `backup.repository`, a generated
   encryption password in `backup.password`, and set `rclone_config` in the
   backup configuration. Keep the password independent of Google credentials.

An external Google OAuth app left in **Testing** issues refresh tokens that
expire after seven days for Drive scopes. Move the separate backup app to the
appropriate production publishing state before relying on unattended backups.
Tokens can still be revoked; backup freshness alerts detect lost access.

## Enable and verify

First complete [controller and updater setup](operations.md#installation-order),
which creates `/opt/infuser/current`. Install a stable operations binary from
that working release:

```sh
sudo install -d -m 0755 /usr/local/libexec
sudo install -m 0755 /opt/infuser/current/infuser /usr/local/libexec/infuser-operations
sudo /usr/local/libexec/infuser-operations backup-init /etc/infuser/backup.toml
sudo install -m 0644 deploy/infuser-backup.service deploy/infuser-backup.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl start infuser-backup.service
journalctl -u infuser-backup.service -n 30
sudo systemctl enable --now infuser-backup.timer
```

`backup-init` is only for a new empty repository; do not reinitialize an existing
backup. A successful backup writes `/var/lib/infuser-backup/last-healthy.json`.
Enable `backup_work_dir` in the [monitoring configuration](monitoring-operations.md).

The operations binary is intentionally separate from the auto-updated daemon.
After a release changes backup code, controller configuration or stored data,
verify it with your configuration and restore checks, then explicitly adopt the
working release. Unlike the updater, this worker interprets platform state and
Docker volumes; ignoring unknown configuration or data would risk an incomplete
backup. Use the [helper adoption procedure](deployment.md#separately-installed-helpers)
under the host maintenance lock, and retain the previous executable for recovery.
Do not assume a daemon update has updated the backup worker or its systemd units.

The timer retries approximately hourly. It defers while games or publishing are
active. After acquiring an admission lease, it stops the controller, checks that
managed containers are stopped, and copies platform state, configuration, all
owned local world volumes and required Docker images. It resumes the controller
before remote transfer and verification. The service's exit hook also resumes
after a killed worker. This is an offline consistent snapshot, not a promise
that copying an arbitrary running game's database is safe.

Staging and verification require space for roughly two uncompressed snapshots
plus the configured reserve. Temporary directories from a killed worker are
removed under the next backup's exclusive lock. World data and published builds
are never deleted to make room. Continuous play can defer backups indefinitely;
an operator must arrange an idle window if freshness alerts persist.

`extra_paths` accepts static configuration directories. Include any private
Caddy, relay or Compose configuration stored outside `/etc/infuser`. Do not put
live unrelated databases there. The snapshot does not back up the OS, all Docker
containers or the full Docker cache. Save the shared-service installation recipe
with your recovery credentials. Certificates can be reissued.

## Restore on a clean host

Keep the original installation stopped throughout activation. First restore
into a new empty directory, never over an existing installation.

1. Install compatible Docker, restic and an Infuser release matching the snapshot
   (or a release known to read its state). Recover the private repository address,
   password and rclone configuration. Set `RESTIC_REPOSITORY_FILE`,
   `RESTIC_PASSWORD_FILE` and `RCLONE_CONFIG` to those files.
2. Run `restic snapshots --json`. Select the verified snapshot. Its `paths` entry
   gives the original staging directory. Restore that subtree:
   `restic restore SNAPSHOT_ID:ORIGINAL_STAGING_PATH --target /srv/infuser-restore --verify`.
3. Run `infuser backup-verify /srv/infuser-restore`. This checks every recorded
   file and directory, permissions, ownership, SQLite integrity and settled
   controller state. Do not activate a snapshot that fails verification.
4. Read `snapshot.json`. If `images.tar` exists, run
   `docker image load -i /srv/infuser-restore/images.tar`. For each `volumes` entry,
   create its exact volume name with labels `org.infuser.controller=OWNER` and
   `org.infuser.game=GAME`, using `owner` and that entry's `game` from the manifest.
   Inspect the new volume's mountpoint and use `cp -a` to copy the entry's `path`
   contents there. Refuse any pre-existing volume rather than overwrite it.
5. Restore `state/` and `config/` with ownership preserved. Recreate the original
   numeric service UID/GID, or deliberately migrate ownership before starting.
   Review private paths, domain, DNS and secrets for the replacement host. Restore
   any numbered `extra/` directories using your original `extra_paths` mapping.
6. Recreate shared services from your private configuration, install the systemd
   units and firewall, then start Infuser. Keep update and backup timers disabled
   until the website works, a saved game loads and a persistent-world game starts
   with its existing data. Enable timers and take a new verified backup afterward.

The local `last-healthy.json` is a convenience, not the only recovery index.
Restic snapshots contain the manifest needed to inspect the backup on a new host.
During a failed replacement there may be more than one snapshot: restore and
verify candidates before deciding which one to activate.

References: [restic restore](https://restic.readthedocs.io/en/stable/050_restore.html),
[rclone Drive scopes](https://rclone.org/drive/#scopes),
[Google Drive authorization](https://developers.google.com/workspace/drive/api/guides/api-specific-auth),
[Google refresh token expiry](https://developers.google.com/identity/protocols/oauth2#expiration).
