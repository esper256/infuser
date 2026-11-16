#!/usr/bin/env bash
# Linux Docker acceptance; no production credentials, domains, ports or volumes.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d)
name="infuser-service-test-$$"
image=${INFUSER_TEST_CADDY_IMAGE:-infuser-caddy:acceptance}
cleanup(){ docker rm -f "$name" >/dev/null 2>&1 || true; rm -rf -- "$work"; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
docker compose -f "$root/deploy/services/compose.yaml" config --format json > "$work/services.json"
login=$(jq -r '.services["oauth2-proxy"].image' "$work/services.json")
turn=$(jq -r '.services.turn.image' "$work/services.json")
docker run --rm --network none --memory 128m --pids-limit 64 "$login" --version
# coturn prints help with exit 255; distinguish that from a failed executable.
turn_status=0
docker run --rm --network none --memory 128m --pids-limit 64 --entrypoint turnserver "$turn" --help > "$work/turn-help" 2>&1 || turn_status=$?
[[ "$turn_status" == 0 || "$turn_status" == 255 ]]
grep -qi 'usage' "$work/turn-help"
docker run --rm --network none "$image" caddy list-modules | grep -Fx dns.providers.porkbun
cat > "$work/Caddyfile" <<'CADDY'
{
  admin off
  auto_https off
}
:8080 {
  respond /healthz "image-acceptance"
  respond /private "Unauthorized" 401
}
CADDY
chmod 0644 "$work/Caddyfile"
docker run -d --name "$name" --read-only --cap-drop ALL --security-opt no-new-privileges \
  --memory 128m --cpus 0.25 --pids-limit 64 --tmpfs /config --tmpfs /data \
  -p 127.0.0.1::8080 -v "$work/Caddyfile:/etc/caddy/Caddyfile:ro" "$image" >/dev/null
port=$(docker port "$name" 8080/tcp | sed 's/.*://')
for attempt in $(seq 1 30); do
  if curl --fail --silent "http://127.0.0.1:$port/healthz" > "$work/response"; then break; fi
  sleep 1
done
test "$(cat "$work/response")" = image-acceptance
test "$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:$port/private")" = 401
echo 'Service image smoke passed: DNS module, HTTP serving, denial response, OAuth and TURN executables.'
