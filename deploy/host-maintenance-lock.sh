#!/usr/bin/env bash
# Serialize backup/reboot/host-config jobs, including slow backup uploads.
# The updater takes this same lock internally after staging its download.
set -euo pipefail
exec 9>"${INFUSER_HOST_LOCK:-/run/lock/infuser-host-maintenance.lock}"
if ! flock -n 9; then
  echo 'Host maintenance deferred: another maintenance job is running.'
  exit 0
fi
exec "$@"
