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
mkdir -p "$work/sites-enabled"
cat > "$work/Caddyfile" <<'CADDY'
{
  auto_https off
}
CADDY
cat "$root/deploy/Caddyfile.host.example" >> "$work/Caddyfile"
cat > "$work/sites-enabled/infuser.caddy" <<'CADDY'
http://infuser.example.test:8080 {
  respond /healthz "image-acceptance"
  respond /private "Unauthorized" 401
}
CADDY
cat > "$work/sites-enabled/other.caddy" <<'CADDY'
http://other.example.test:8080 {
  respond "other-project"
}
CADDY
chmod -R a+rX "$work/sites-enabled"
chmod 0644 "$work/Caddyfile"
docker run -d --name "$name" --read-only --cap-drop ALL --security-opt no-new-privileges \
  --memory 128m --cpus 0.25 --pids-limit 64 --tmpfs /config --tmpfs /data \
  -p 127.0.0.1::8080 -v "$work/Caddyfile:/etc/caddy/Caddyfile:ro" \
  -v "$work/sites-enabled:/etc/caddy/sites-enabled:ro" "$image" >/dev/null
port=$(docker port "$name" 8080/tcp | sed 's/.*://')
for attempt in $(seq 1 30); do
  if curl --fail --silent -H 'Host: infuser.example.test' "http://127.0.0.1:$port/healthz" > "$work/response"; then break; fi
  sleep 1
done
test "$(cat "$work/response")" = image-acceptance
test "$(curl --silent -H 'Host: infuser.example.test' --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:$port/private")" = 401
test "$(curl --fail --silent -H 'Host: other.example.test' "http://127.0.0.1:$port/private")" = other-project
mv "$work/sites-enabled/other.caddy" "$work/sites-enabled/other.disabled"
docker exec "$name" caddy reload --config /etc/caddy/Caddyfile >/dev/null 2>&1
test -z "$(curl --silent -H 'Host: other.example.test' "http://127.0.0.1:$port/")"
test "$(curl --fail --silent -H 'Host: infuser.example.test' "http://127.0.0.1:$port/healthz")" = image-acceptance
mv "$work/sites-enabled/other.disabled" "$work/sites-enabled/other.caddy"
docker exec "$name" caddy reload --config /etc/caddy/Caddyfile >/dev/null 2>&1
test "$(curl --fail --silent -H 'Host: other.example.test' "http://127.0.0.1:$port/")" = other-project
echo 'Service image smoke passed: independent domains, site reload isolation, DNS module, OAuth and TURN executables.'
