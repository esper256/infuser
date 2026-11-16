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
urls = ["turn:games.example.com:8700?transport=udp", "turn:games.example.com:8700?transport=tcp"]
secret_file = "/etc/infuser/turn.secret"
relay_port_start = 8800
relay_port_end = 8899
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

## Adult diagnostics

The SDK sends bounded browser connection reports every five seconds, stored only
in memory for the active peer session. Adult accounts can inspect them under
Admin tools → Network diagnostics. `GET /api/admin/peer-health` requires adult
Google membership; the equivalent loopback-only `GET /v1/peer-health` uses the
existing operator bearer credential. Reports include selected candidate types,
byte counters, and the browser's current round-trip time estimate when available.
These are diagnostic observations, not proof of device performance or blame for
lag. Reporting failures never stop gameplay. The diagnostics contract remains
experimental and is separate from game state and cloud saves.
