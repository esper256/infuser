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
2. Install a stable copy of the working binary and prepare the metrics directory:

   ```sh
   sudo install -d -m 0755 /usr/local/libexec /var/lib/infuser-monitor/textfile
   sudo install -m 0755 /opt/infuser/current/infuser /usr/local/libexec/infuser-operations
   sudo install -m 0644 deploy/infuser-health.service deploy/infuser-health.timer /etc/systemd/system/
   sudo systemctl daemon-reload
   sudo /usr/local/libexec/infuser-operations doctor /etc/infuser/operations.toml
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

The stack includes a dashboard and alert rules. In **Alerting → Contact points**,
configure the destination you use, send a test notification, and select it in the
notification policy. Until that succeeds, alerts are visible in Grafana but
outside notifications are **not configured**. Export contact points and policies
into the private provisioning directory if they must be recovered with the host
configuration; exported files may contain credentials. Grafana history is not
part of the game backup.

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

The collector keeps a current report, not an incident database. Inspect failures
with `journalctl -u infuser-health -u infuser --since '15 minutes ago'`. Container
logs remain the source for dependency failures. Reinstall the stable operations
binary when adopting changes to these commands.

## Whole-host outages

This stack cannot notify you after its own VM loses power or network access.
Run an HTTPS uptime check on another machine or an external monitoring service,
with a notification destination independent of Infuser. Do not expose internal
metrics or the controller token to that probe.

References: [Grafana alerting](https://grafana.com/docs/grafana/latest/alerting/set-up/configure-alertmanager/),
[node_exporter textfiles](https://github.com/prometheus/node_exporter#textfile-collector),
[Prometheus retention](https://prometheus.io/docs/prometheus/latest/storage/).
