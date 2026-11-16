#!/usr/bin/env bash
# Render private Grafana provisioning; does not contact any external service.
set -euo pipefail
umask 077
if [[ $# != 2 ]]; then
  echo 'Usage: render-notifications.sh PRIVATE_CONFIG_JSON OUTPUT_JSON' >&2
  exit 2
fi
source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
target=$2
staged=$(mktemp "${target}.XXXXXX")
trap 'rm -f -- "$staged"' EXIT
jq --slurpfile base "$source_dir/provisioning/alerting/infuser.json" '
  def url: type == "string" and (test("^https://[^/[:space:]]+/[^[:space:]]+$") or test("^http://127\\.0\\.0\\.1:[0-9]+/[^[:space:]]+$")) and (contains("REPLACE")|not);
  # Grafana interpolates environment variables in provisioning settings.
  def literal: gsub("\\$"; "$$");
  if type != "object" or (keys - ["webhook_url","webhook_bearer_token","heartbeat_url"]|length) != 0
    then error("Unknown notification configuration fields") else . end |
  if (.webhook_url|url|not) then error("Set a valid HTTPS webhook_url") else . end |
  (.webhook_bearer_token // "") as $token |
  (.heartbeat_url // "") as $heartbeat |
  if ($token|type) != "string" or ($token|test("[\r\n]")) then error("Invalid webhook token") else . end |
  if $heartbeat != "" and ($heartbeat|url|not) then error("Set a valid HTTPS heartbeat_url or leave it empty") else . end |
  {apiVersion:1,
   contactPoints:[{orgId:1,name:"infuser-operator",receivers:[{
     uid:"infuser-operator-webhook",type:"webhook",disableResolveMessage:false,
     settings:({url:(.webhook_url|literal),httpMethod:"POST",maxAlerts:"20",
       title:"Infuser · {{ if eq .Status \"firing\" }}needs attention{{ else }}recovered{{ end }}",
       message:"{{ range .Alerts.Firing }}Problem: {{ .Labels.alertname }}{{ with .Labels.check }} ({{ . }}){{ end }}\n{{ .Annotations.summary }}\n{{ end }}{{ range .Alerts.Resolved }}Recovered: {{ .Labels.alertname }}{{ with .Labels.check }} ({{ . }}){{ end }}\n{{ end }}"}
       + if $token == "" then {} else {authorization_scheme:"Bearer",authorization_credentials:($token|literal)} end)
   }]}],
   policies:[{orgId:1,receiver:"infuser-operator",group_by:["service"],
     group_wait:"45s",group_interval:"5m",repeat_interval:"4h",
     routes:[]}]
  } |
  if $heartbeat == "" then
    .deleteRules=[{orgId:1,uid:"infuser-watchdog"}]
  else
    .contactPoints += [{orgId:1,name:"infuser-heartbeat",receivers:[{
      uid:"infuser-heartbeat-webhook",type:"webhook",disableResolveMessage:true,
      settings:{url:($heartbeat|literal),httpMethod:"POST",maxAlerts:"1",
        title:"Infuser monitoring heartbeat",message:"Monitoring pipeline is running."}
    }]}] |
    .policies[0].routes=[{receiver:"infuser-heartbeat",object_matchers:[["route","=","watchdog"]],
      group_by:["service"],group_wait:"0s",group_interval:"1m",repeat_interval:"1m"}] |
    .groups=[{orgId:1,name:"Infuser watchdog",folder:"Infuser",interval:"1m",rules:[
      ($base[0].groups[0].rules[0] |
        .uid="infuser-watchdog" | .title="Monitoring heartbeat" | .for="0s" |
        .noDataState="OK" | .execErrState="OK" |
        .labels={service:"infuser",route:"watchdog",severity:"none"} |
        .annotations={summary:"Expected to fire while the monitoring pipeline is healthy. Never route this to a person."} |
        .data[0].model.expr="(min(up{job=~\"host|prometheus|grafana|website\"}) == 1) and on() (count(up{job=~\"host|prometheus|grafana|website\"}) >= 4) and on() (time() - max(infuser_collector_timestamp_seconds) < 180)")
    ]}]
  end
' "$1" > "$staged"
chmod 0600 "$staged"
mv -f -- "$staged" "$target"
echo 'Private notification provisioning written. No notifications sent.'
