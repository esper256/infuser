# Install the player platform

The `infuser` daemon serves the clubhouse, public publishing API, static games,
and player services on its existing loopback port. Front it with Caddy using
[`deploy/Caddyfile.example`](../deploy/Caddyfile.example). Do not expose the daemon
port or route `/v1/*` publicly. Configure an apex and wildcard DNS record and
wildcard certificate. Existing legacy `render-caddy` output is for operator-staged
sites and is not the player-platform routing template.

## Google login and invitations

Run OAuth2 Proxy bound to `127.0.0.1:4180`, provider Google, scopes `openid email
profile`, and `set_xauthrequest=true`. Configure the Google redirect URI as
`https://YOUR_DOMAIN/oauth2/callback`. Use secure host-only cookies named
`__Host-infuser_session`, path `/`, HttpOnly, SameSite Lax. Do **not** share the
Google cookie across game subdomains. Keep client ID/secret and cookie key in a
root-readable private environment file, never in GitHub.

Infuser owns the invitation list. Configure OAuth2 Proxy with `email_domains=["*"]`
so it validates Google identities, then Infuser checks each request against its
own enabled members. An uninvited Google account may authenticate with Google
but cannot see games, launch, publish, or read platform data. Do not also retain
an old `authenticated_emails_file`, or new Infuser invitations will fail upstream.
Use a static upstream (`static://202`) because Caddy routes `/oauth2/*` separately
and the platform calls `/oauth2/auth` directly to validate sessions.

Bootstrap an adult using the local CLI, as the service user so database ownership
and SQLite WAL files remain consistent:

```sh
sudo -u infuser /usr/local/bin/infuser member /etc/infuser/infuser.toml adult@example.com 'Grown-up' --admin
```

The same command can add members or apply `--disable`. Adults can subsequently
invite/disable members in the clubhouse. Use a person's intended display name;
it is what friends see online and alongside reactions.

## Firewall helper

Install/enable the default-closed firewall first, as in [network-admission.md](network-admission.md).
Install [`infuser-firewall-agent.service`](../deploy/infuser-firewall-agent.service)
and enable it. It runs a tiny Unix-socket service with only CAP_NET_ADMIN. The
main daemon does not get a network-administration capability. Configure:

```toml
[controller]
firewall_socket = "/run/infuser-firewall/admit.sock"
content_budget_mb = 2048
```

Caddy must **overwrite** `X-Infuser-Client-IP` with `{remote_host}`. Successful
member requests and active game heartbeats grant/refresh 30-minute nftables
entries, rate-limited per address. OAuth-cookie validation alone is insufficient:
Infuser membership must also pass. Shared public IPs share access; native servers
still validate game tickets. A logout does not revoke another family's shared
IP; grants expire in the kernel when they stop being refreshed.

Use `MemoryMax=512M` for the main daemon with publishing enabled (uploads are
serialized and capped at 64 MiB/file). The existing 8 GiB game budget leaves host
headroom on a 12 GiB example installation; choose budgets for your own hardware.
The helper only needs 64 MiB. Publishing content has an aggregate disk budget,
independent of the game's world volumes and Docker images.

## Data and updates

Back up the controller state directory, including `platform.sqlite` with its WAL
(or use SQLite's backup API), `content/`, and `server-keys/`, plus Docker world
volumes. Do not copy only the main SQLite file while writes are active. Stop the
service briefly for a simple filesystem backup.

Platform rows are additive and the new daemon reads old controller state. Once
publishing-managed servers exist, do not roll back to pre-platform binaries that
cannot read their catalog. Keep a pre-upgrade backup for such a manual rollback.
The current auto-updater still checks health and retains the previous binary.

Unpublished builds are retained intentionally so an operator can recover or
republish them. Remove unused builds through the API to reclaim disk. Guest-save
cleanup runs during library refresh; personal saves survive unpublish and updates.
Admin-only preview means hidden from ordinary members, not a separate server.
