#!/usr/bin/env bash
# Internal worker. run.sh places it in a finite systemd unit with recovery.
set -euo pipefail
umask 077
[[ $# == 1 ]] || exit 2
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
runtime=${INFUSER_HOST_RUNTIME:-/run/infuser-host-apply}
port=${INFUSER_API_PORT:-7777}
unit=${INFUSER_SERVICE_UNIT:-infuser.service}
[[ "$port" =~ ^[0-9]+$ && "$unit" =~ ^[a-zA-Z0-9_.@-]+\.service$ ]] || exit 2
[[ -d "$runtime" ]] || { echo 'Run through run.sh apply.' >&2; exit 2; }
exec 9>"${INFUSER_HOST_LOCK:-/run/lock/infuser-host-maintenance.lock}"
flock -n 9 || { echo 'Apply deferred: another host maintenance job is running.'; exit 75; }
# A stale recovery marker requires recovery before another attempt.
[[ ! -e "$runtime/resume.json" ]] || { echo 'Recover the previous interrupted apply first.' >&2; exit 1; }
"${INFUSER_ANSIBLE_BIN:-/opt/infuser/ansible/bin/ansible-playbook}" \
  -i "$1" "$here/host.yml" --syntax-check
token=$(<"${INFUSER_CONTROL_TOKEN_FILE:-/etc/infuser/control.secret}")
[[ "$token" =~ ^[A-Za-z0-9_-]{32,}$ ]] || exit 2
lease=
request(){
  printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --config - --silent --show-error --max-time 8 --request "$1" \
      --header 'Content-Type: application/json' --data-binary "$2" \
      --output "$runtime/admission.json" --write-out '%{http_code}' "http://127.0.0.1:$port/v1/maintenance"
}
cleanup(){
  local code=$?
  trap - EXIT
  "$here/recover.sh" --locked || code=1
  if [[ -n "$lease" ]]; then request DELETE "$(jq -nc --arg id "$lease" '{id:$id}')" >/dev/null || true; fi
  rm -f -- "$runtime/admission.json"
  exit "$code"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
code=$(request POST '{"purpose":"update","force":false}')
[[ "$code" != 409 ]] || { echo 'Apply deferred: games, publishing or maintenance are active.'; exit 75; }
[[ "$code" == 200 ]] || { echo "Admission failed (HTTP $code); nothing applied." >&2; exit 1; }
lease=$(jq -er '.lease.id | strings | select(length>0)' "$runtime/admission.json")
expires=$(jq -er '.lease.expires | numbers' "$runtime/admission.json")
health_timer=false
if systemctl is-active --quiet infuser-health.timer; then health_timer=true; fi
# Mark before stopping so systemd's independent recovery covers abrupt worker exit.
jq -nc --arg unit "$unit" --argjson port "$port" --argjson health_timer "$health_timer" \
  '{unit:$unit,port:$port,health_timer:$health_timer}' > "$runtime/resume.json"
# The collector can restart an unresponsive controller; suspend that recovery
# during intentional downtime, preserving whether its timer was running.
if [[ $(systemctl show infuser-health.service --property=LoadState --value) == loaded ]]; then
  systemctl stop infuser-health.timer infuser-health.service
fi
(( expires - $(date +%s) >= 15 )) || { echo 'Admission nearly expired; retry apply.' >&2; exit 1; }
systemctl stop "$unit"
[[ $(systemctl show "$unit" --property=ActiveState --value) == inactive ]] || {
  echo 'Controller did not stop; nothing applied.' >&2; exit 1;
}
# Once stopped, lease expiry cannot admit new work during a long service update.
export INFUSER_HOST_GUARDED=1
"${INFUSER_ANSIBLE_BIN:-/opt/infuser/ansible/bin/ansible-playbook}" \
  -i "$1" "$here/host.yml"
