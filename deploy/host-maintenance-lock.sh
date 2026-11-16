#!/usr/bin/env bash
# Serialize whole backup/update/reboot jobs, including slow backup uploads.
set -euo pipefail
exec 9>"${INFUSER_HOST_LOCK:-/run/lock/infuser-host-maintenance.lock}"
if ! flock -n 9; then
  echo 'Host maintenance deferred: another maintenance job is running.'
  exit 0
fi
exec "$@"
