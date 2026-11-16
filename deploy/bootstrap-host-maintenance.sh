#!/usr/bin/env bash
set -euo pipefail
[[ $EUID == 0 && $# == 1 ]] || { echo 'Usage: sudo bash deploy/bootstrap-host-maintenance.sh TIME_ZONE' >&2; exit 2; }
zone=$1
[[ "$zone" =~ ^[A-Za-z0-9_+/-]+$ && "$zone" != /* && -f "/usr/share/zoneinfo/$zone" ]] || { echo 'Unknown time zone.' >&2; exit 2; }
for command in curl jq flock systemctl systemd-analyze; do command -v "$command" >/dev/null; done
source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
calendar="*-*-* 04:00,05,10,15,20,25:00 $zone"
systemd-analyze calendar "$calendar"
for unit in infuser-backup.service infuser-update.service; do
  state=$(systemctl show "$unit" --property=ActiveState --value)
  [[ "$state" == inactive || "$state" == failed ]] || { echo "Wait for $unit to finish, then retry." >&2; exit 1; }
done
test -f /etc/infuser/control.secret
if [[ -f /etc/infuser/reboot.conf ]]; then
  grep -Fx "INFUSER_MAINTENANCE_TIMEZONE=$zone" /etc/infuser/reboot.conf >/dev/null || {
    echo 'Existing reboot.conf uses another zone; align it with the requested zone before retrying.' >&2; exit 1;
  }
else
  install -m 0640 "$source_dir/reboot.example.conf" /etc/infuser/reboot.conf
  sed -i "s|^INFUSER_MAINTENANCE_TIMEZONE=.*|INFUSER_MAINTENANCE_TIMEZONE=$zone|" /etc/infuser/reboot.conf
fi
install -d /usr/local/libexec /etc/systemd/system/infuser-reboot.timer.d \
  /etc/systemd/system/infuser-backup.service.d /etc/systemd/system/infuser-update.service.d
install -m 0755 "$source_dir/host-maintenance-lock.sh" /usr/local/libexec/infuser-host-maintenance-lock
install -m 0755 "$source_dir/reboot-if-required.sh" /usr/local/libexec/infuser-reboot-if-required
install -m 0644 "$source_dir/infuser-reboot.service" "$source_dir/infuser-reboot.timer" /etc/systemd/system/
install -m 0644 "$source_dir/maintenance-lock-backup.conf" /etc/systemd/system/infuser-backup.service.d/maintenance-lock.conf
install -m 0644 "$source_dir/maintenance-lock-update.conf" /etc/systemd/system/infuser-update.service.d/maintenance-lock.conf
printf '[Timer]\nOnCalendar=\nOnCalendar=%s\n' "$calendar" > /etc/systemd/system/infuser-reboot.timer.d/schedule.conf
install -m 0644 "$source_dir/99-infuser-reboots" /etc/apt/apt.conf.d/99-infuser-reboots
systemctl daemon-reload
systemctl enable --now infuser-reboot.timer
systemctl list-timers infuser-reboot.timer --no-pager
echo 'Required-reboot scheduling enabled. No reboot was requested by installation.'
