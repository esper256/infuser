#!/usr/bin/env bash
# Thin invocation wrapper: Ansible owns configuration, systemd owns job lifetime.
set -euo pipefail
[[ $# == 2 && ( $1 == plan || $1 == apply ) ]] || {
  echo 'Usage: sudo deploy/ansible/run.sh plan|apply /absolute/private/inventory.yml' >&2; exit 2;
}
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
inventory=$(realpath -- "$2")
ansible=${INFUSER_ANSIBLE_BIN:-/opt/infuser/ansible/bin/ansible-playbook}
[[ -f "$inventory" && -x "$ansible" ]] || { echo 'Inventory or Ansible executable missing.' >&2; exit 2; }
export ANSIBLE_CONFIG="$here/ansible.cfg"
export ANSIBLE_COLLECTIONS_PATH="${ANSIBLE_COLLECTIONS_PATH:-/opt/infuser/ansible/collections}"
if [[ $1 == plan ]]; then
  exec "$ansible" -i "$inventory" "$here/host.yml" --check --diff
fi
[[ $EUID == 0 ]] || { echo 'Apply requires root on the managed host.' >&2; exit 2; }
# No --pipe: output stays in journald and the job survives the SSH session.
# A stable unit name prevents overlapping applies. No timer is installed.
exec systemd-run --unit=infuser-host-apply --collect --wait \
  --property=Type=exec --property=UMask=0077 \
  --property=RuntimeDirectory=infuser-host-apply --property=RuntimeDirectoryMode=0700 \
  --property=RuntimeDirectoryPreserve=yes \
  --property=TimeoutStopSec=90 --property=RuntimeMaxSec=30min \
  --property="ExecStopPost=$here/recover.sh" \
  --setenv="ANSIBLE_CONFIG=$ANSIBLE_CONFIG" \
  --setenv="ANSIBLE_COLLECTIONS_PATH=$ANSIBLE_COLLECTIONS_PATH" \
  --setenv="INFUSER_ANSIBLE_BIN=$ansible" \
  --setenv="INFUSER_API_PORT=${INFUSER_API_PORT:-7777}" \
  --setenv="INFUSER_CONTROL_TOKEN_FILE=${INFUSER_CONTROL_TOKEN_FILE:-/etc/infuser/control.secret}" \
  --setenv="INFUSER_SERVICE_UNIT=${INFUSER_SERVICE_UNIT:-infuser.service}" \
  "$here/apply.sh" "$inventory"
