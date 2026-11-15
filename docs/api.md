# Lifecycle API

The Rust daemon binds to `127.0.0.1:7777` by default. A trusted website backend
will authenticate the player, enforce membership, and request a lease on their
behalf. Never give a browser or game the control token. These routes are versioned
independently from the future public publishing API.

| Method | Route | Credential | Purpose |
| --- | --- | --- | --- |
| GET | `/healthz` | None | Process liveness, not Docker readiness |
| GET | `/v1/apps` | Control token | States and reserved resources |
| GET | `/v1/capabilities` | Control token | Explicit supported multiplayer modes and publishing availability |
| POST | `/v1/apps/{app_id}/leases` | Control token | Launch or join; body `{"session_id":"stable-player-session-id"}` |
| GET | `/v1/apps/{app_id}/leases/{lease_id}` | Lease token | Poll readiness; does not renew |
| PUT | `/v1/apps/{app_id}/leases/{lease_id}` | Lease token | Heartbeat; no body |
| DELETE | `/v1/apps/{app_id}/leases/{lease_id}` | Lease token | Leave; no body |
| DELETE | `/v1/apps/{app_id}/instance` | Control token | Stop app and revoke leases |

Credentials use `Authorization: Bearer TOKEN`. Launch returns `202` while starting
and `200` when ready. Repeating a launch with the same `session_id` returns the same
unexpired lease and token. A different player/session gets a separate lease, while
sharing the same game process. The backend should generate a unique session ID
for each player/device, rather than trusting arbitrary user-supplied identities.

```json
{
  "app_id": "moon",
  "state": "starting",
  "endpoint": null,
  "lease_id": "opaque-id",
  "lease_token": "opaque-secret",
  "expires_at": 1800000090,
  "heartbeat_after_seconds": 30,
  "active_leases": 1,
  "error": null,
  "retry_at": 0
}
```

Renew during startup as well as play. Poll until `state` is `ready`; only then
does `endpoint` appear. Native endpoints contain `host`, `port`, and `protocols`.
Web endpoints contain an HTTPS `url`; use its WSS equivalent for WebSockets.
Lease status responses never return a lease token. Keep tokens out of URLs/logs.

Expired leases cannot be renewed. Request a fresh launch after expiration or a
server failure. `409` means stopping or no longer running, `410` means expired,
`429` means that app's session limit, and `503` means total capacity or failure
backoff. Clients should retry these transient admission errors with bounded
backoff. The daemon does not evict games with active leases to admit another game.

Idle means no live player leases, not low CPU or a lack of packets. A game server
must not renew an empty world's lease forever. Background browser timer throttling
can expire a browser lease; the future website backend can own renewal for
confirmed active sessions. Persistent volumes survive sleep, but the game must
checkpoint on SIGTERM before its configured shutdown deadline.

Direct TCP/UDP game ports are not protected by website cookies. The game protocol
must authenticate players before admitting them. The future public player API
will provide game-bound admission tickets; do not treat an open port or the
control API's launch permission as an authenticated in-game connection. Until
that integration is installed, bind native servers to loopback/private networking.
