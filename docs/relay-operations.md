# Run the shared multiplayer services

Audience: server operators.

## Installing the shared relay

Run coturn alongside the controller, with the same private shared-secret file.
See [the configuration template](../deploy/turnserver.example.conf). Use an
ARM64-compatible image where appropriate, pinned to a tested digest. With the
current official coturn image, retain `NET_BIND_SERVICE` when dropping other
capabilities: the executable has a file capability. Run as a non-root UID, with
read-only storage, a small writable `/tmp`, CPU/RAM/PID limits, host networking,
and log rotation. The relay does not need the Docker socket.

Reserve non-overlapping listener and relay ports outside the game allocator.
For example, reduce controller `port_end` to 8699 and add:

```toml
[controller.turn]
engine = "coturn" # or "embedded"
urls = ["turn:games.example.com:8700?transport=udp", "turn:games.example.com:8700?transport=tcp"]
secret_file = "/etc/infuser/turn.secret"
relay_port_start = 8800
relay_port_end = 8899
# Required only for engine = "embedded". Same mapping as coturn external-ip.
# listen_ip = "10.0.0.5"
# public_ip = "203.0.113.10"
# socket = "/var/lib/infuser/turn.sock"
```

Generate a random shared secret locally on the server. Protect it and the coturn
config from other users; never commit either. Allow the listener (UDP/TCP) and
relay range (UDP) in both the cloud and host firewall. Keep the installation's
login-based IP admission rules covering these ports. Set coturn `external-ip`
when behind a public/private IP mapping. Check the provider's NAT hairpin behavior
when both peers use this same relay: allocations must reach each other's public
relay addresses. Any necessary hairpin rule should match only this server's IP
and the reserved relay ports, not all outbound traffic.

The example uses TURN UDP/TCP, with WebRTC DTLS protecting game payloads; it does
not configure TURN-over-TLS. TLS-only traversal is currently unsupported.

## Embedded relay

`engine = "embedded"` runs the relay inside `infuser turn`, a second process
from the same binary. It listens on the same 8700 UDP/TCP ports and relays on
the same UDP range. It does not open any new public port and it does not change
the host firewall. The daemon and the relay share a Unix socket under the state
directory. The relay asks the daemon whether a peer is still admitted, and it
sends back the bindings and allocations used for the player-bar label.

Install the unit once, from a checkout that contains it:

```sh
sudo install -m 0644 deploy/infuser-turn.service /etc/systemd/system/
sudo systemctl daemon-reload
```

Leave the unit disabled until cutover. The updater restarts `infuser-turn.service`
only when the new binary's relay fingerprint differs from the previous binary.
A website or game-server deploy does not restart it, so relayed matches stay up.
That check lives in the updater binary at `/usr/local/libexec/infuser-updater`.
Install the updater from this release before expecting the automatic restart.
Until then, restart the unit yourself when a release says the relay changed.

Replace `/usr/local/libexec/infuser-updater` with the new `infuser` binary only
as part of the documented updater install. Do not point the updater at a
half-written file.

### Cutover

1. Add `engine = "embedded"`, `listen_ip`, and `public_ip` to `[controller.turn]`.
   `listen_ip` is the address coturn bound. `public_ip` is the address in
   coturn's `external-ip`. Keep the same secret file and the same ports.
2. `sudo systemctl enable --now infuser-turn.service` will fail while coturn
   still holds the ports. Stop the container without deleting it:
   `sudo docker stop infuser-turn`.
3. `sudo systemctl start infuser-turn.service` and `sudo systemctl restart infuser.service`.
4. In `/etc/infuser/operations.toml`, remove `infuser-turn` from `containers`
   and add `infuser-turn.service` to `services`, then let the next collector
   run. A stopped container would otherwise stay red.
5. Play a relay-only match with two authorized players. The bar should say
   Relayed, and Admin → Network diagnostics should show bytes on the allocation.

### Rollback

1. Set `engine = "coturn"`.
2. `sudo systemctl stop infuser-turn.service` so the ports are free.
3. `sudo docker start infuser-turn`.
4. `sudo systemctl restart infuser.service`.
5. Put `infuser-turn` back on the container list and remove
   `infuser-turn.service` from `services`.

The coturn container and its config stay on disk. Rollback does not recreate
them. A binary older than this release cannot read `engine`, `listen_ip`,
`public_ip`, or `socket`. Remove those keys before switching back to such a
binary.
To check the relay, use two authorized test players with
`relayOnly: true`. Look for **relay ↔ relay** in selected-candidate stats and
increasing byte counters. Merely gathering a relay candidate does not prove
gameplay used it.

## PeerJS signaling

One shared private PeerServer stays running for the installation; it does not
start per game. Games declare the adapter, and rooms form when they enter
multiplayer. PeerServer introduces players; TURN carries relayed game traffic.
Infuser authenticates the players before allowing signaling. Keep PeerServer
private and do not expose its discovery or ID-generation endpoints.

With TURN already installed, build the PeerServer container and bind it **only to
loopback**. Use a checkout matching the daemon release:

```sh
docker build -t infuser-peerjs services/peerjs
# Pin the resulting local image ID, not a mutable tag, for the running container.
image=$(docker image inspect --format '{{.Id}}' infuser-peerjs)
docker run -d --name infuser-peerjs --restart unless-stopped \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  --memory 128m --cpus 0.25 --pids-limit 64 \
  --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  --log-opt max-size=5m --log-opt max-file=2 \
  -p 127.0.0.1:7780:9000 "$image"
```

Add `peerjs_port = 7780` inside `[controller]` in the private server config, then
restart the daemon. Requires a daemon release with PeerJS support. The existing
Caddy routing covers `/_infuser/peerjs/peerjs`; no new public ports or DNS records
are needed. Leave the PeerServer port outside game/TURN allocations. A missing
sidecar fails closed; there is no public-server fallback. Existing Infuser and
Godot peer games keep their original adapters and require no changes.

## Admin diagnostics

The SDK sends bounded browser connection reports every five seconds, stored only
in memory for the active peer session. Admin accounts can inspect them under
Admin tools → Network diagnostics. `GET /api/admin/peer-health` requires admin
Google membership; the equivalent loopback-only `GET /v1/peer-health` uses the
existing operator bearer credential. Reports include selected candidate types,
byte counters, and the browser's current round-trip time estimate when available.
These are diagnostic observations, not proof of device performance or blame for
lag. Reporting failures never stop gameplay. The diagnostics contract remains
experimental and is separate from game state and cloud saves.
