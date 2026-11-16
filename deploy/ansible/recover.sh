#!/usr/bin/env bash
# Runs both on normal completion and independently as systemd ExecStopPost.
set -euo pipefail
runtime=${INFUSER_HOST_RUNTIME:-/run/infuser-host-apply}
[[ -e "$runtime/resume.json" ]] || exit 0
if [[ ${1:-} != --locked ]]; then
  exec 9>"${INFUSER_HOST_LOCK:-/run/lock/infuser-host-maintenance.lock}"
  flock -w 60 9 || { echo 'Controller recovery could not acquire the host lock.' >&2; exit 1; }
fi
unit=$(jq -er '.unit' "$runtime/resume.json")
port=$(jq -er '.port' "$runtime/resume.json")
[[ "$unit" =~ ^[a-zA-Z0-9_.@-]+\.service$ && "$port" =~ ^[0-9]+$ ]] || exit 2
systemctl daemon-reload
systemctl start "$unit"
if [[ $(jq -r '.health_timer // false' "$runtime/resume.json") == true ]]; then
  systemctl start infuser-health.timer
fi
curl --fail --silent --show-error --max-time 5 --retry 10 --retry-delay 1 \
  --retry-connrefused "http://127.0.0.1:$port/healthz" >/dev/null
rm -f -- "$runtime/resume.json"
echo 'Infuser controller resumed and healthy.'
