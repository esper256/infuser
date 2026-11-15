#!/usr/bin/env bash
set -euo pipefail

# Replace only our table, atomically. Restarting clears grants and fails closed.
batch=$(mktemp)
trap 'rm -f "$batch"' EXIT
if /usr/sbin/nft list table inet infuser_ingress >/dev/null 2>&1; then
    printf 'delete table inet infuser_ingress\n' > "$batch"
fi
cat /etc/infuser/game-ingress.nft >> "$batch"
/usr/sbin/nft --check --file "$batch"
/usr/sbin/nft --file "$batch"
