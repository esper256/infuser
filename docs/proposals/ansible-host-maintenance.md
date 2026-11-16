# Host maintenance with Ansible

Audience: Infuser developers.

Status: implemented. Disposable Ansible journeys and a real ARM64 Compose 5.7.1
journey passed with ansible-core 2.20.11 and community.docker 5.4.0. The first
guarded systemd apply completed successfully on an existing installation;
deployment-specific inventory and verification logs remain private.

## Decisions

1. Use Ansible for configuration management, not a new Rust installer or an
   installation-state database. Compose remains the container definition and
   execution layer. The automatic controller updater remains independent.
2. Manage an explicit file list and explicitly selected fixed Compose services.
   Never synchronize entire configuration directories, remove orphan containers,
   delete volumes, adopt dynamic game containers or rewrite Grafana's shared
   notification policy tree.
3. Keep private inventory, credentials, service overrides, schedules and data
   outside releases. Release templates supply defaults; inventory can replace a
   particular managed file with a private source. Secret-bearing copies suppress
   contents in output, including check/diff mode.
4. Check mode previews file changes without touching running services. It is not
   a saved transaction or a complete prediction of image changes. Apply uses the
   exact checked-out release and inventory; rerun the preview if either changes.
5. Apply runs locally on the host in a finite systemd job, launched over SSH if
   desired. This holds the existing flock for the whole job and survives a lost
   operator connection. No permanent Ansible service, UI, database or timer.
6. Acquire existing non-forced idle admission, then stop the controller before
   changes. The current admission lease expires after 60 seconds; stopping avoids
   accidentally admitting games during a longer Compose build or restart. A
   recovery marker and independent ExecStopPost restart the controller after an
   unexpected worker exit. Busy installations defer without changing config.
7. Retain Ansible's timestamped configuration backups and pinned release sources.
   Reapply a previous image definition/release to revert software. This is not an
   atomic multi-service rollback, and never rewinds game data. Incompatible data
   migrations still require a separate backup and recovery plan.

## Scope and ownership

The first implementation adopts an existing update-managed Ubuntu installation.
Fresh host bootstrap continues through the installation guide. It manages service
units, tracked service image/build sources, monitoring defaults and optionally
Infuser's Caddy fragment. Operator-owned files include OAuth/TURN secrets, Compose
mounts/environment, monitoring targets, notification destinations and reboot zone
settings. Other projects use separate site fragments, scrape files and providers.

The stable updater and backup/recovery executables are not silently replaced by
configuration maintenance. Their explicit update procedure remains in their
operator guides. OCI policy, Drive authorization and game resource budgets are
outside this work.

## Apply and failure behavior

Ansible copies only owned files, preserving originals with `backup: true`.
Per-file validation can reject a candidate before replacement. Inventory supplies
combined read-only validators before service reconciliation. An invalid combined
configuration fails the run without restarting containers; an operator restores
reported backups before retrying. Do not claim that Ansible automatically undoes
all earlier tasks.

Changed unit definitions are reloaded; only explicitly enabled timers restart.
Selected Compose services are reconciled without dependencies, orphan removal or
volume removal. Changed bind-mounted configuration causes recreation because an
atomic file replacement can leave a running container on the old inode. Health
checks run before the worker completes; controller recovery runs on success or
failure. A hard host failure still needs normal boot/recovery and backups.

## Acceptance

Exercise real Ansible check/apply/idempotence in a disposable filesystem, retain
an unrelated project's files, prove secret text does not enter diff output, and
verify configuration backup/validation behavior. Exercise the maintenance wrapper
with a local HTTP fixture and fake systemd: busy admission, lock contention,
failed apply, signals and independent recovery. Validate the installed Compose
version through a real isolated stack before relying on its Ansible module.

Adopt production first in check mode with a private inventory. Review differences
against existing overrides before applying; no deployment-specific values belong
in this repository. Record the resulting plan, versions and acceptance privately.

The independent health collector can initiate controller recovery. The apply
worker therefore pauses it before stopping the controller and records its timer's
previous running state in the recovery marker. Recovery restores that state.
Outside probes remain independent; a long maintenance job may alert. Permission-
only file repairs do not trigger image rebuilds or service recreation.
