#!/usr/bin/env bash
# Run through host-maintenance-lock.sh. No package installs or forced shutdown.
set -euo pipefail
umask 077
zone=${INFUSER_MAINTENANCE_TIMEZONE:-Etc/UTC}
flag=${INFUSER_REBOOT_FLAG:-/var/run/reboot-required}
token_file=${INFUSER_CONTROL_TOKEN_FILE:-/etc/infuser/control.secret}
port=${INFUSER_API_PORT:-7777}
[[ $# == 0 || ( $# == 1 && $1 == --check ) ]] || exit 2
[[ "$port" =~ ^[0-9]+$ ]] || exit 2
defer(){ echo "Reboot deferred: $*."; exit 0; }
[[ -f "$flag" ]] || { echo 'No reboot required.'; exit 0; }
# Also guard manual/late starts: a missed timer must never reboot during playtime.
clock=$(TZ="$zone" date +%H%M)
[[ "$clock" =~ ^04[0-2][0-9]$ ]] || defer 'outside the 04:00–04:30 maintenance window'
for unit in apt-daily.service apt-daily-upgrade.service; do
  state=$(systemctl show "$unit" --property=ActiveState --value)
  [[ "$state" == inactive || "$state" == failed ]] || defer 'package maintenance is running'
done
token=$(<"$token_file")
[[ "$token" =~ ^[A-Za-z0-9_-]{32,}$ ]] || { echo 'Invalid controller credential format.' >&2; exit 1; }
work=$(mktemp -d)
lease=
requested=false
request(){
  local method=$1 body=$2
  printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --config - --silent --show-error --max-time 8 --request "$method" \
      --header 'Content-Type: application/json' --data-binary "$body" \
      --output "$work/response" --write-out '%{http_code}' "http://127.0.0.1:$port/v1/maintenance"
}
cleanup(){
  if [[ -n "$lease" && "$requested" == false ]]; then
    request DELETE "$(jq -nc --arg id "$lease" '{id:$id}')" >/dev/null || true
  fi
  rm -rf -- "$work"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
# OS patching uses the existing update admission contract, without its force option.
code=$(request POST '{"purpose":"update","force":false}')
[[ "$code" != 409 ]] || defer 'players, publishing or maintenance are active'
[[ "$code" == 200 ]] || { echo "Maintenance admission failed (HTTP $code); no reboot." >&2; exit 1; }
lease=$(jq -er '.lease.id | strings | select(length > 0)' "$work/response")
[[ -f "$flag" ]] || defer 'the reboot request has been cleared'
clock=$(TZ="$zone" date +%H%M)
[[ "$clock" =~ ^04[0-2][0-9]$ ]] || defer 'the maintenance window has ended'
if [[ ${1:-} == --check ]]; then
  echo 'Reboot would be permitted; check mode released maintenance without rebooting.'
  exit 0
fi
echo 'Ubuntu requires a reboot; Infuser is idle. Requesting an orderly reboot.'
# Respect shutdown inhibitors. Never use --force or bypass normal service shutdown.
systemctl --check-inhibitors=yes reboot
requested=true
# Successful shutdown keeps admission closed until the daemon stops (lease also expires).
