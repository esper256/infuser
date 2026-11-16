# Host monitoring

Audience: server operators.

Infuser exports current health to Prometheus. Grafana OSS supplies charts, alert
rules, notification policies and silences. The website's **Groups → Host health**
view is available only to installation operators, not group admins or players.

## Install

First deploy an Infuser release that includes `/v1/operations`. The independent
collector needs that endpoint; do not enable recovery against an older daemon.

1. Copy `deploy/operations.example.toml` to `/etc/infuser/operations.toml`. Adjust
   shared container names and paths. Leave `backup_work_dir` commented until
   [backups](backup-operations.md) are configured.
2. Install the collector timer and prepare the metrics directory. It runs the
   activated release as a separate process, so it understands the current
   configuration even when the daemon is unavailable:

   ```sh
   sudo install -d -m 0755 /var/lib/infuser-monitor/textfile
   sudo install -m 0644 deploy/infuser-health.service deploy/infuser-health.timer /etc/systemd/system/
   sudo systemctl daemon-reload
   sudo /opt/infuser/current/infuser doctor /etc/infuser/operations.toml
   sudo systemctl enable --now infuser-health.timer
   ```

3. Copy `deploy/monitoring/` into private installation configuration, for example
   `/etc/infuser/monitoring/`. In its `targets.json`, set the website URL:

   ```json
   [{"targets":["https://games.example.com/"],"labels":{"service":"infuser"}}]
   ```

4. Create a private password file for Grafana, readable by its container user
   (UID 472). Set `GRAFANA_ADMIN_PASSWORD_FILE` to its absolute path. Do not use
   your Google password. Check and start the Compose stack:

   ```sh
   sudo --preserve-env=GRAFANA_ADMIN_PASSWORD_FILE docker compose \
     --project-name infuser-monitoring -f /etc/infuser/monitoring/compose.yaml config --quiet
   sudo --preserve-env=GRAFANA_ADMIN_PASSWORD_FILE docker compose \
     --project-name infuser-monitoring -f /etc/infuser/monitoring/compose.yaml up -d
   ```

All listeners bind loopback. No cloud firewall changes or public Grafana route
are needed. From your computer, use `ssh -N -L 3000:127.0.0.1:3000 YOUR_HOST`, then
open `http://127.0.0.1:3000` and sign in as `admin` with the file's password.

The stack includes separate Infuser and Host dashboards. Configure delivery below
to install the eight health rules and optional heartbeat from `alert-rules.json`.
Starting the stack alone does not enable outside notifications. Grafana history
is not part of the game backup.

## Phone notifications and outage detection

Grafana sends problem and recovery notifications to an ntfy topic. A separate
heartbeat goes to Healthchecks.io every minute. If the VM or monitoring pipeline
stops sending, Healthchecks alerts independently of the VM.

1. Subscribe to a private topic in the ntfy phone app and allow notifications.
   Generate an unpredictable topic name, for example with `openssl rand -hex 24`.
   An unreserved topic is accessible to anyone with its name; keep the URL
   private. A reserved/authenticated topic can use a bearer token instead.
2. Create a Healthchecks check with **Period 1 minute**, **Grace Time 4 minutes**.
   Keep email enabled as an independent fallback. Its ntfy integration can also
   send outages to the same phone topic. Copy its ping URL, not an account API key.
3. Copy `deploy/monitoring/notifications.example.json` into a root-only directory
   outside the checkout, such as `/etc/infuser/monitoring-private/notifications.json`.
   Replace `webhook_url` with your topic URL followed by `?template=grafana`.
   Set `heartbeat_url` to the check's ping URL. Leave `webhook_bearer_token` empty
   unless your destination requires it. Restrict the directory to mode 0700 and
   the file to 0600. Never commit this file or the rendered output.
4. Render and install the private provisioning file; the renderer requires Bash
   and jq and makes no network requests:

   ```sh
   sudo bash deploy/monitoring/render-notifications.sh \
     /etc/infuser/monitoring-private/notifications.json \
     /etc/infuser/monitoring-private/rendered-notifications.json
   sudo install -o 472 -g 0 -m 0400 \
     /etc/infuser/monitoring-private/rendered-notifications.json \
     /etc/infuser/monitoring/provisioning/alerting/notifications.json
   ```

5. Restart only Grafana using the same Compose command and password-file setting
   as installation, with `restart grafana` instead of `up -d`. For an existing
   installation, also copy the current `prometheus.yml`, `compose.yaml` and
   `scrape.d/` example and recreate only
   Prometheus with `up -d --no-deps --force-recreate prometheus`. This adds the
   project labels and the scrape-directory mount. Keep your private `targets.json`
   and other projects' files. See the upgrade note below before replacing older
   alert provisioning.
6. In Grafana **Alerting → Contact points**, test **infuser-operator**. Confirm
   receipt on the phone. In Healthchecks, confirm recurring pings and a healthy
   check. Test its notification integration too; provider acceptance alone does
   not prove the phone has notifications enabled.

The generated rules select their contact points directly. The renderer does not
provision or reset Grafana's shared notification policy tree, so other projects
can manage their own routing without an Infuser update replacing it.
Edit the private input and render again to change destinations; provisioned
settings are managed by files. Preserve those files in your private host backup.
An empty `heartbeat_url` disables the watchdog rule, leaving ordinary alerts.

### Upgrade from the original dedicated monitoring setup

Back up private provisioning and export the current notification policy tree
before changing it. The new generated `notifications.json` includes the health
rules as well as contacts: remove the old deployed `alerting/infuser.json` after
rendering, so the same rules are not provisioned twice. Keep all unrelated files.
Install the new dashboard subdirectories/provider file too; dashboard UIDs and
the existing Prometheus datasource UID remain stable.

Removing a policy from its provisioning file does not undo the already installed
policy. If the old tree contains only Infuser's default route and watchdog route,
reset that legacy tree once through Grafana's provisioning API after the new
direct notification routes are active. If the tree also contains another
project's routing, preserve it and migrate deliberately; never reset it blindly.
Future Infuser provisioning must not include `policies` or `resetPolicies`.

## Adding another project

Use the existing Prometheus/Grafana installation. Put a unique scrape job in
`scrape.d/PROJECT.yml`, following `project.yml.example`, and label its targets
`project: PROJECT`. Keep the built-in job names `host`, `prometheus`, `grafana`
and `website` reserved. Infuser's website job always receives `project: infuser`;
its application metrics keep the `infuser_` prefix, receive `project="infuser"`,
and are read only from the host collector job. Standard `node_*` host metrics
receive `project="host"` and are shared, not copied for each project. Host memory,
disk and core scrape alerts remain in the Host folder.

Validate new files with `promtool check config /etc/prometheus/prometheus.yml`
inside the Prometheus container, then send that container `SIGHUP` to reload.
The scrape-directory mount allows new jobs without recreating the container.
The original `targets.json` remains the Infuser website list. For another site's
HTTPS check, create a separate blackbox scrape job with its own project label.

Add dashboards through a uniquely named provider under `provisioning/dashboards/`
and its own directory under `dashboards/`, using that project's folder and unique
dashboard/rule UIDs. Reload Grafana provisioning after adding providers or rules.
Other projects can select their own contacts directly or use the shared policy
tree. The existing `infuser-operator` contact receives only the explicitly routed
Infuser and host checks; its historical name does not make it a global default.
No extra Grafana or Prometheus process, public port, or Infuser Google login is
needed. Project labels organize metrics; they are not a security boundary.

Problems are grouped, with a 45-second initial delay, subsequent changes at
five-minute intervals, and reminders every four hours. Recovery messages are
enabled. Payloads contain operational check names and summaries, not player
names, chat, saves or raw logs.

The **Monitoring heartbeat** rule is intentionally always firing while scrapes
and the collector are healthy. It goes only to Healthchecks, never to the phone
topic. Missing data or query errors stop it; resolved messages are disabled so a
failure cannot masquerade as a successful ping. Do not silence this rule.

## What is checked

- API, database and lifecycle-loop responsiveness; Docker and shared services.
- Installed daemon limits, restart activity, failed deployment and long deferral.
- Available disk, application storage categories, host memory and CPU.
- Last restore-verified backup and failures, when backups are enabled.
- Website response, HTTPS certificate expiry, failed scrapes and stale collector.

Persistent failures alert after a short grace period. Missing collector data is
an alert, not a successful check. Backup staging is explicitly reported; a long
maintenance window has its own alert. No account names, saves, chat content or
game titles appear in metrics. Counts and disk categories are bounded.

The independent collector can restart a running but unresponsive daemon after
three failed checks, at most once per five minutes. It does not restart while a
backup is staging, when Docker is unavailable, or when the daemon is deliberately
stopped. Set `recover_unresponsive_daemon = false` for observation only. Systemd
handles crashed processes; `PartOf=docker.service` restarts Infuser when Docker
is restarted. Binary upgrades do not update installed service units.

The included stack caps memory and CPU, limits container logs, and retains seven
days of metrics with a 1 GB TSDB block target. The WAL and current head need extra
space, so that target is not a disk quota. Monitor actual use on your host. Do not
add Loki or a second Alertmanager unless you need their extra capabilities.

The collector keeps a current report, not an incident database. The separate
stable binary used for backup recovery is described in the backup guide; the
health collector follows `/opt/infuser/current/infuser` automatically. Reinstall
the health unit when upgrading from an older unit that used the stable binary.

## Responding to an alert

- **Dependency or website:** open **Groups → Host health** for the failing check
  and suggested action. If the website is unavailable, inspect
  `journalctl -u infuser-health -u infuser --since '15 minutes ago'` over SSH,
  then the affected container's logs. Check Caddy and DNS for HTTPS failures.
- **Collector or monitoring target:** check `systemctl status infuser-health.timer
  infuser-health.service` and the monitoring Compose services. A failed collector
  can mean configuration drift; it does not necessarily mean games have stopped.
- **Disk, memory or certificate:** inspect actual filesystem use, container
  memory and Caddy renewal logs. Do not prune game volumes or saves to free space.
- **Long backup staging:** inspect the backup unit, staging space and maintenance
  status. Its stop hook should resume Infuser after a failure. Avoid force-starting
  games while a backup is still copying their state.
- **Healthchecks missed heartbeat:** check cloud VM/network status, SSH access,
  the collector, Prometheus and Grafana. Inspect the heartbeat contact point's
  last delivery attempt. An outbound/provider failure can also cause this alert.

For planned work, silence only the affected health alerts with an expiry. If
stopping the monitoring stack or VM, pause the Healthchecks check deliberately
and resume it afterward. A broad silence that includes the watchdog will cause
an external missed-heartbeat alert.

## Whole-host outages

Healthchecks detects missing heartbeats without relying on the VM to deliver an
alert. The website probe itself still runs on the VM: successful heartbeats do
not prove inbound HTTPS is reachable from another network. An independently
hosted HTTPS probe is an optional additional check for that failure mode. It
needs only the public website URL, never internal metrics or a controller token.

References: [Grafana alerting](https://grafana.com/docs/grafana/latest/alerting/set-up/configure-alertmanager/),
[file provisioning](https://grafana.com/docs/grafana/latest/alerting/set-up/provision-alerting-resources/file-provisioning/),
[ntfy Grafana template](https://docs.ntfy.sh/publish/#message-templating),
[Healthchecks](https://healthchecks.io/docs/),
[node_exporter textfiles](https://github.com/prometheus/node_exporter#textfile-collector),
[Prometheus retention](https://prometheus.io/docs/prometheus/latest/storage/).
