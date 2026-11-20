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
# listen_ip = "REPLACE_PRIVATE_IP"
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
the host firewall. The daemon pushes the admitted-peer set over a Unix socket
under the state directory. The relay checks that set locally and sends back a
full snapshot of bindings and allocations when the socket connects, then again
every few seconds. Nonces are stateless and last 600 seconds, the same window
as coturn `stale-nonce`.

`listen_ip` is this machine's private address, the one coturn bound. The
example placeholder is `REPLACE_PRIVATE_IP`. Put the real address there. Do not
leave a documentation address if it is not this host.

Install the unit once, from a checkout that contains it:

```sh
sudo install -m 0644 deploy/infuser-turn.service /etc/systemd/system/
sudo systemctl daemon-reload
```

Leave the unit disabled until cutover. The updater restarts `infuser-turn.service`
only when the binary it is activating does not match `turn.fingerprint` in the
state directory, which the running relay writes at startup. A failed restart is
part of the deploy outcome, and the next deploy tries again while that file
still differs. A website or game-server deploy does not restart the relay, so
relayed matches stay up. The check lives in the updater binary at
`/usr/local/libexec/infuser-updater`. Install the updater from this release
before expecting the automatic restart. Until then, restart the unit yourself
when a release says the relay changed.

Replace `/usr/local/libexec/infuser-updater` with the new `infuser` binary only
as part of the documented updater install. Do not point the updater at a
half-written file.

### Cutover

Do this only after the release you are running and the release you would roll
back to both contain this relay. A binary from before that cannot read
`engine`, `listen_ip`, `public_ip`, or `socket`, and the daemon would refuse
the config. Removing the keys is not a substitute: both builds have to
understand them.

1. Add `engine = "embedded"`, `listen_ip`, and `public_ip` to `[controller.turn]`.
   `listen_ip` is the address coturn bound. `public_ip` is the address in
   coturn's `external-ip`. Keep the same secret file and the same ports.
2. Stop the container without deleting it, and keep it from coming back on a
   reboot: `sudo docker update --restart=no infuser-turn` then
   `sudo docker stop infuser-turn`.
3. `sudo systemctl enable --now infuser-turn.service` and
   `sudo systemctl restart infuser.service`.
4. In `/etc/infuser/operations.toml`, move `infuser-turn` from `containers` to
   `services` as `infuser-turn.service`, then let the next collector run. A
   stopped container would otherwise stay red.
5. Play a relay-only match with two authorized players. The bar should say
   Relayed, and Admin → Network diagnostics should show bytes on the allocation.

If `engine` is not `embedded`, the unit exits 78 and systemd does not restart
it. That keeps it from grabbing 8700 while coturn is the engine.

### Rollback

1. Set `engine = "coturn"`.
2. `sudo systemctl disable --now infuser-turn.service` so the ports are free
   and a reboot does not start the unit.
3. `sudo docker update --restart=unless-stopped infuser-turn` and
   `sudo docker start infuser-turn`.
4. `sudo systemctl restart infuser.service`.
5. Put `infuser-turn` back on the container list and remove
   `infuser-turn.service` from `services`.

The coturn container and its config stay on disk. Rollback does not recreate
them. A binary older than this release cannot read `engine`, `listen_ip`,
`public_ip`, or `socket`. Do not roll back to that binary while those keys
are in the config.
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
