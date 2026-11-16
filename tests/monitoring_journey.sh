#!/usr/bin/env bash
# Opt-in, real notification routing. Requires Linux Docker, curl and jq; no Rust or npm install.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d)
project="infuser-monitor-test-$$"
web=${INFUSER_TEST_WEBHOOK_PORT:-19095}
prom=${INFUSER_TEST_PROMETHEUS_PORT:-19090}
grafana=${INFUSER_TEST_GRAFANA_PORT:-13000}
cleanup(){
  docker rm -f "$project-grafana" "$project-prometheus" "$project-fixture" >/dev/null 2>&1 || true
  rm -rf -- "$work"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
for port in "$web" "$prom" "$grafana"; do [[ "$port" =~ ^[0-9]+$ ]]; done
mkdir -p "$work/provisioning/alerting" "$work/provisioning/datasources"
printf '%s\n' 'fixture-password' > "$work/password"
GRAFANA_ADMIN_PASSWORD_FILE="$work/password" docker compose -f "$root/deploy/monitoring/compose.yaml" config --format json > "$work/compose.json"
grafana_image=$(jq -r '.services.grafana.image' "$work/compose.json")
prometheus_image=$(jq -r '.services.prometheus.image' "$work/compose.json")
# Shorter evaluation/grace periods keep the same expressions and routing testable in minutes.
jq '.groups[].interval="10s" | .groups[].rules[].for="10s"' "$root/deploy/monitoring/provisioning/alerting/infuser.json" > "$work/provisioning/alerting/infuser.json"
jq -n --arg url "http://127.0.0.1:$web/alerts" --arg heartbeat "http://127.0.0.1:$web/heartbeat" --arg token 'fixture$token' \
  '{webhook_url:$url,webhook_bearer_token:$token,heartbeat_url:$heartbeat}' > "$work/input.json"
"$root/deploy/monitoring/render-notifications.sh" "$work/input.json" "$work/rendered.json"
jq '.policies[0].group_wait="0s" | .policies[0].group_interval="10s" | .policies[0].repeat_interval="1h" |
    .policies[0].routes[0].group_interval="10s" | .policies[0].routes[0].repeat_interval="10s" |
    .groups[].interval="10s"' "$work/rendered.json" > "$work/provisioning/alerting/notifications.json"
cat > "$work/provisioning/datasources/prometheus.yml" <<YAML
apiVersion: 1
datasources:
  - name: Prometheus
    uid: infuser-prometheus
    type: prometheus
    access: proxy
    url: http://127.0.0.1:$prom
    isDefault: true
YAML
cat > "$work/prometheus.yml" <<YAML
global:
  scrape_interval: 2s
  scrape_timeout: 1s
scrape_configs:
  - job_name: host
    static_configs: [{targets: ['127.0.0.1:$web']}]
  - job_name: website
    static_configs: [{targets: ['127.0.0.1:$web']}]
  - job_name: prometheus
    static_configs: [{targets: ['127.0.0.1:$prom']}]
  - job_name: grafana
    static_configs: [{targets: ['127.0.0.1:$grafana']}]
YAML
# Only fixture credentials live here. The enclosing temporary directory stays private.
chmod -R a+rX "$work/provisioning"
chmod 0644 "$work/prometheus.yml"
common=(--detach --network host --read-only --cap-drop ALL --security-opt no-new-privileges --pids-limit 128)
docker run "${common[@]}" --name "$project-fixture" --memory 96m --cpus 0.1 \
  -v "$root/tests/monitoring_fixture.mjs:/fixture.mjs:ro" --entrypoint node node:24-bookworm-slim /fixture.mjs "$web" >/dev/null
docker run "${common[@]}" --name "$project-prometheus" --memory 192m --cpus 0.25 --tmpfs /prometheus:mode=777,size=64m \
  -v "$work/prometheus.yml:/etc/prometheus/prometheus.yml:ro" "$prometheus_image" \
  --config.file=/etc/prometheus/prometheus.yml --web.listen-address="127.0.0.1:$prom" --storage.tsdb.path=/prometheus >/dev/null
docker run "${common[@]}" --name "$project-grafana" --memory 384m --cpus 0.35 \
  --tmpfs /var/lib/grafana:mode=777,size=64m --tmpfs /tmp:mode=777,size=16m \
  -v "$work/provisioning:/etc/grafana/provisioning:ro" \
  -e GF_SERVER_HTTP_ADDR=127.0.0.1 -e "GF_SERVER_HTTP_PORT=$grafana" \
  -e GF_SECURITY_ADMIN_PASSWORD=fixture-password -e GF_ANALYTICS_REPORTING_ENABLED=false \
  -e GF_ANALYTICS_CHECK_FOR_UPDATES=false -e GF_PLUGINS_PREINSTALL_DISABLED=true \
  "$grafana_image" >/dev/null
wait_for(){
  local message=$1;shift
  for attempt in $(seq 1 90); do if "$@"; then return; fi;sleep 1;done
  echo "Timed out: $message" >&2
  docker logs --tail 15 "$project-grafana" >&2
  return 1
}
normal(){
  curl -fsS --max-time 2 -u admin:fixture-password "http://127.0.0.1:$grafana/api/prometheus/grafana/api/v1/rules" 2>/dev/null |
    jq -e '[.data.groups[].rules[]|select(.name!="Monitoring heartbeat")|.state=="inactive"] | length == 8 and all' >/dev/null
}
event(){ curl -fsS --max-time 2 "http://127.0.0.1:$web/events" | jq -e "$1" >/dev/null; }
state(){ curl -fsS --max-time 2 -X POST -d "$1" "http://127.0.0.1:$web/state" >/dev/null; }
wait_for 'all baseline rules healthy' normal
wait_for 'initial heartbeat' event 'any(.path=="/heartbeat" and .body.status=="firing")'
curl -fsS -X POST "http://127.0.0.1:$web/reset" >/dev/null
state '{"failed":true}'
wait_for 'failure notification' event 'any(.path=="/alerts" and any(.body.alerts[]; .status=="firing" and .labels.check=="controller"))'
state '{"failed":false}'
wait_for 'recovery notification' event 'any(.path=="/alerts" and any(.body.alerts[]; .status=="resolved" and .labels.check=="controller"))'
state '{"stale":true}'
watchdog_stopped(){
  curl -fsS --max-time 2 -u admin:fixture-password "http://127.0.0.1:$grafana/api/prometheus/grafana/api/v1/rules" |
    jq -e '[.data.groups[].rules[]|select(.name=="Monitoring heartbeat")|.state] == ["inactive"]' >/dev/null
}
wait_for 'stale collector stops the heartbeat' watchdog_stopped
heartbeats(){ curl -fsS "http://127.0.0.1:$web/events" | jq '[.[]|select(.path=="/heartbeat")]|length'; }
before=$(heartbeats)
sleep 22
[[ $(heartbeats) == "$before" ]]
state '{"stale":false}'
wait_for 'pipeline recovery resumes the heartbeat' event "[.[]|select(.path==\"/heartbeat\")]|length > $before"
event 'all(.path!="/heartbeat" or .body.status=="firing")'
echo 'Monitoring journey passed: real firing/recovery delivery, token handling, heartbeat routing, stale-pipeline cutoff and recovery.'
