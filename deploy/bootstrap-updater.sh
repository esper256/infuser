#!/usr/bin/env bash
set -euo pipefail

if [ "$EUID" -ne 0 ] || [ "$#" -ne 1 ]; then
  echo 'Usage: sudo bash deploy/bootstrap-updater.sh /path/to/working/infuser' >&2
  exit 1
fi

binary=$(realpath "$1")
deploy_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
test -x "$binary"
test -f /etc/infuser/infuser.toml
test -f /etc/infuser/update.toml
"$binary" check /etc/infuser/infuser.toml

install -d -m 0755 /opt/infuser/releases /usr/local/libexec
if [ -e /opt/infuser/current ] || [ -L /opt/infuser/current ]; then
  echo 'An update-managed installation already exists; leaving it unchanged.' >&2
  exit 1
fi
install -d -m 0755 /opt/infuser/releases/bootstrap
install -m 0755 "$binary" /opt/infuser/releases/bootstrap/infuser
"$binary" version > /opt/infuser/releases/bootstrap/build-id
install -m 0755 "$binary" /usr/local/libexec/infuser-updater
ln -s /opt/infuser/releases/bootstrap /opt/infuser/current
# The running executable remains intact; the next service restart follows this link.
ln -s /opt/infuser/current/infuser /usr/local/bin/infuser.next
mv -Tf /usr/local/bin/infuser.next /usr/local/bin/infuser
install -m 0644 "$deploy_dir/infuser-update.service" /etc/systemd/system/
install -m 0644 "$deploy_dir/infuser-update.timer" /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now infuser-update.timer
echo 'Automatic updates enabled. Configuration and credentials stay on this host.'
