# Maintain host configuration with Ansible

Audience: server operators.

Use this after [installing Infuser](operations.md). It maintains explicitly owned
configuration and fixed Compose services on an existing host. The controller's
automatic binary updater continues independently. No new background service or
public port is required.

## Install the operator tools

On the server, use a source checkout/archive from the release you intend to adopt.
Keep it at a stable, root-owned path for the duration of the job and any rollback.
The examples below use `/opt/infuser/host-maintenance/RELEASE`.

```sh
sudo apt-get install python3-venv
sudo python3 -m venv /opt/infuser/ansible
sudo /opt/infuser/ansible/bin/pip install -r /opt/infuser/host-maintenance/RELEASE/deploy/ansible/requirements.txt
sudo env ANSIBLE_COLLECTIONS_PATH=/opt/infuser/ansible/collections \
  /opt/infuser/ansible/bin/ansible-galaxy collection install \
  -r /opt/infuser/host-maintenance/RELEASE/deploy/ansible/requirements.yml
sudo install -d -m 0700 /etc/infuser/ansible-private
sudo install -m 0600 /opt/infuser/host-maintenance/RELEASE/deploy/ansible/inventory/example.yml \
  /etc/infuser/ansible-private/inventory.yml
```

The pinned Ansible series supports Ubuntu's Python 3.12. This tooling is for
operators, not players or game-author agents. Bash, curl, jq and util-linux are
also required by the existing maintenance integration.

Edit the private inventory before running. Select the existing Compose project
names, absolute directories, files, services and health endpoints. The example
paths are conventions, not discovery. Set `infuser_enabled_timers` to the timers
already enabled on your host; leave the backup timer out until storage is ready.
Inventory values must be trusted operator input: file destinations and validators
run with host privileges.

## Preview

```sh
sudo /opt/infuser/host-maintenance/RELEASE/deploy/ansible/run.sh plan \
  /etc/infuser/ansible-private/inventory.yml
```

This uses Ansible check/diff mode. It does not replace installed configuration or
change running services. Private notification rules are rendered into a temporary
private directory to compare them, then removed. Secret-bearing file contents
and Compose environment values are suppressed; paths and change status remain
visible. Keep the output private because domains and deployment paths may appear.

The preview reports file changes and Compose's prediction for currently installed
stack definitions. It cannot fully predict a new image or stack definition that
has not been installed. Rerun after changing release sources or inventory. This
is not an immutable saved plan or an atomic transaction.

The playbook owns only its listed files. Private targets, secrets, reboot zone
settings, controller configuration, other projects' site/scrape files and game
state remain outside that list. To retain a customized managed file, add an
`infuser_extra_files` entry with the same destination and a private source. Do not
edit both the installed output and its source independently.

Stable updater and backup executables have an explicit
[adoption procedure](deployment.md#separately-installed-helpers). Remove temporary
`infuser_extra_files` entries for those binaries after adoption; otherwise a later
routine apply can reinstall an obsolete helper from its pinned source.

`infuser_caddy_domain` optionally renders the public site template into only
`/etc/caddy/sites-enabled/infuser.caddy`. Keep it empty for a customized provider
or routing configuration and supply a private file override instead. It never
rewrites the shared root Caddyfile. Do not use both options for the same fragment.

## Validate and apply

Use per-file `validate` commands where possible; Ansible validates its temporary
candidate before replacement. Add combined read-only commands under
`infuser_validators` for Caddy/Prometheus and other multi-file configurations.
They run after file installation and before container reconciliation. Their
output is hidden because it can contain private configuration.

```sh
sudo /opt/infuser/host-maintenance/RELEASE/deploy/ansible/run.sh apply \
  /etc/infuser/ansible-private/inventory.yml
sudo journalctl -u infuser-host-apply.service --since '15 minutes ago'
```

Apply runs as a finite systemd job. A disconnected SSH session does not cancel
it. It takes the existing backup/update/reboot lock, requires idle admission and
stops the controller while applying changes. Busy admission or lock contention
exits with status 75; retry later. There is no forced interruption option.

The health collector's timer is temporarily paused so its automatic recovery
cannot restart the controller mid-update; its previous running state is restored.
A long maintenance job can trigger outside availability/heartbeat alerts. Check
mode never pauses monitoring. An independent systemd recovery hook resumes the
controller after a worker failure. The whole job is limited to 30 minutes.

Selected services may restart, and image builds/pulls can extend downtime. Apply
is an explicit maintenance operation, even when the file preview is clean. No
scheduled apply is installed. Game containers are never selected, no Compose
orphans or volumes are removed, and dependencies outside each selected service
list are not started.

For non-default controller settings, run with root-preserved environment values
`INFUSER_API_PORT`, `INFUSER_CONTROL_TOKEN_FILE` and `INFUSER_SERVICE_UNIT`.
The token variable contains a file path, never the token. `INFUSER_ANSIBLE_BIN`
can select another installed Ansible executable. Changing the controller port
or service identity during this maintenance run is not supported.

## Failure and rollback

Ansible prints the paths of timestamped backup files when replacing existing
configuration. Preserve those files and the previous release sources/image IDs
until the new setup is verified. New files have no previous copy. An ordinary
Ansible failure does not undo earlier successful tasks; inspect the journal and
restore the affected backups before retrying. Restore permissions along with
contents, and validate the combined configuration again.

Reapplying older pinned image definitions can restore supporting software.
For Caddy's locally built image, retain its previous image ID or rebuild from
that previous release's locked sources. Neither action restores application data;
state-changing upgrades require their own recovery plan. The backup and updater
executables still use their separate documented adoption procedures.

An interrupted apply retains `/run/infuser-host-apply/resume.json` until controller
recovery succeeds. Inspect the failed unit and run the release's `recover.sh` as
root if recovery needs retrying. That helper restarts the controller and restores
monitoring; it does not silently roll back configuration. Never delete the marker
just to suppress a failure.

Standalone containers are not implicitly adopted. Put them in an explicit Compose
project first, or retain their documented operator procedure. The source image
catalog alone does not mean the playbook manages every running container.
