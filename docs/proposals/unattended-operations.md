# Unattended operation

Audience: Infuser developers.

Status: implementation, automated acceptance, and ARM64 deployment complete.
Backup destinations, cloud snapshot policy, notification delivery, and independent
uptime setup are deferred at the operator's request. Continue with the
[SDK 1.0 stabilization plan](sdk-1.0-stabilization.md).

## Delivery order

1. Recoverable backups: coordinate an idle maintenance window, stage coherent
   platform/config/world/image data, resume service before remote transfer, then
   encrypt with restic. Restore into a fresh directory and verify it before
   declaring success. Keep only the latest verified snapshot; an unsuccessful
   replacement must not remove the previous healthy snapshot. Use a dedicated
   repository so retention cannot affect unrelated backups.
2. Storage accounting and conservative cleanup: report categories, protect active
   and retained published data, and remove only known disposable remnants. Never
   blindly prune Docker volumes or delete saves to free space.
3. Failure/endurance journeys: exercise interrupted maintenance, daemon recovery,
   dependency failures and repeated publication/start/stop cycles in disposable
   installations. Longer runs are explicit, not a new PR gate.
4. Operator health: distinguish process liveness from usable dependencies. Export
   low-cardinality metrics to Prometheus; use Grafana OSS's built-in alerting for
   persistent problems and recovery. Expose useful status only to operators.
   Avoid periodic success-log spam or player-facing telemetry.
5. Safer deployment: use the same maintenance coordination, defer while players
   are active, retain explicit operator override and automatic rollback, and
   detect installed configuration/service drift.

## Backup decisions

No remote destination is configured yet. The requested destination is the
operator's Google Drive quota, with a separate OAuth app requesting only
`drive.file`. Rclone can provide that transport; restic provides encryption and
integrity rather than Infuser implementing either. The limited refresh token and
backup password stay in private operator files, outside source control. Keep an
independent recovery copy of the password. Authorizing Drive remains a user step.

Only one healthy snapshot is retained. A replacement and its predecessor can
coexist while verification is in progress. Retention runs only after successful
restore verification; partial/failed uploads never count as a healthy backup.
Remote deletion must reclaim space rather than accumulate superseded data in
Drive trash. This policy intentionally provides no historical recovery versions.

Default maintenance waits for idle games and publishing. No generic live copy
of a game-owned database is claimed to be consistent. The first implementation
copies worlds only after all managed runtimes have stopped normally. Rebuilding
managed images from npm is not disaster recovery: required local image contents
must be included or otherwise recoverable by exact digest.

The daemon resumes after local staging, before slow network upload or restore
verification. A service recovery hook must resume it if the backup worker exits
unexpectedly. A maintenance lease expires if its caller disappears before stop.

## Monitoring decision

Use Prometheus, node_exporter, blackbox_exporter and Grafana OSS. Infuser supplies
application checks and a small bounded recovery helper, not a second alerting
engine or metrics database. A root-owned timer outside the daemon exports a
sanitized textfile. A separate stale-data alert catches a failed collector.
Grafana owns notification grouping, retries, silences and recovery notifications.
The first deployment keeps the dashboards on loopback, accessible by SSH tunnel;
the website's operator tools expose a small current-health view.

Start with seven days of metrics and a 1 GB TSDB block retention target, explicit
container limits and bounded logs. WAL/head data require additional disk space;
retention is not a hard filesystem quota. Measure actual ARM64 memory use before
adjusting caps. Journal and existing container logs suffice; defer Loki and a
separate Alertmanager until they solve a demonstrated need.

Private provisioning now routes Grafana problems/recoveries to ntfy and a
one-minute watchdog to Healthchecks.io. The watchdog stops on failed scrapes,
stale collection or evaluation errors; Healthchecks alerts independently when
pings stop. Email remains an independent fallback for missed heartbeats.
The collector follows the activated binary to avoid configuration-version drift.
An outside HTTPS probe remains optional: outbound heartbeats do not prove inbound
reachability from another network. See [operator setup](../monitoring-operations.md).

## Evidence

Initial production audit: release aa334f3 is active, the 512 MiB/128-task limits
are installed, the daemon reports zero restarts, and disk use is low. Private host
details and subsequent deployment measurements stay in `.local/`.

Local evidence: fast Rust and SDK tests, operator browser journey and real Caddy
journeys pass. The restic fixture restores game assets and saves through HTTP and
keeps one verified snapshot. Recovery tests exercise stalled lifecycle checks,
cooldown and the backup exit hook with missing source paths. The actual
Prometheus/Grafana stack loads the dashboard and all eight alert rules and passed
a real failure/firing/recovery cycle; its local AMD64 test used approximately
355 MB combined. The manual Docker lane passed 20 wake/serve/sleep cycles, world
ownership and image snapshots, encrypted restore, managed Node hosting and Caddy.
ARM64 deployment passed health checks and uses approximately 321 MiB combined for
the four monitoring containers. The opt-in monitoring journey additionally
passed real notification delivery, recovery, bearer-token handling and watchdog
cutoff/resumption on stale data. Live Grafana test delivery reached ntfy, and
scheduled watchdog delivery to Healthchecks succeeded. Phone receipt remains an
operator acceptance step. Private rollout details are recorded separately.

References: [restic repositories](https://restic.readthedocs.io/en/stable/030_preparing_a_new_repo.html),
[restoring backups](https://restic.readthedocs.io/en/stable/050_restore.html),
[Google Drive scopes](https://developers.google.com/workspace/drive/api/guides/api-specific-auth),
[rclone Drive scopes](https://rclone.org/drive/#scopes).
