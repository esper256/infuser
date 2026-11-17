# Set up the website and Google login

Audience: server operators.

First install the controller using [the installation guide](operations.md).
This page adds the website, login and player access.

The daemon serves the Library, public publishing API, static games,
and player services on its existing loopback port. Use
[`deploy/Caddyfile.host.example`](../deploy/Caddyfile.host.example) as Caddy's
root configuration and an edited copy of
[`deploy/Caddyfile.example`](../deploy/Caddyfile.example) as
`/etc/caddy/sites-enabled/infuser.caddy`. Keep global Caddy options in the root
file. Follow [shared domain setup](host-maintenance.md#sharing-a-host-with-other-projects)
for mounts, validation and reloads. Do not expose the daemon
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

Use the session settings in
[`deploy/oauth2-proxy.example.cfg`](../deploy/oauth2-proxy.example.cfg):
`cookie_expire="168h"`, `cookie_refresh="0s"` and `session_cookie_minimal=true`.
Infuser uses Google to verify sign-in, not to call Google APIs. These settings
keep a fixed seven-day login and leave OAuth tokens out of new cookies. Reloads
and other tabs share that login. Keep all token-forwarding options disabled.
Do not enable periodic refresh: Google can return a successful sign-in without
a refresh token, causing those sessions to fail when its access token expires.

Signing out clears the browser's login. Disabling an Infuser account or profile
still denies access immediately; revoking Google's grant alone does not revoke
an already issued Infuser login before its seven-day expiry. See OAuth2 Proxy's
[session and cookie options](https://oauth2-proxy.github.io/oauth2-proxy/configuration/overview/).
For an existing installation, preserve the cookie secret and name when adopting
these settings. Existing signed cookies remain usable until their normal expiry.
Apply the configuration through the host's maintenance procedure and recreate
the login container so it sees the updated bind-mounted file.

Set `prompt="select_account"` explicitly. Otherwise OAuth2 Proxy's legacy default
can send `approval_prompt=force`, asking Google to repeat consent during login.
Account selection is useful on shared laptops; it does not force another consent
grant. Verify the actual `/oauth2/start` redirect contains `prompt=select_account`
and no `approval_prompt=force` or `prompt=consent`. First-time authorization and
Google's own account/security prompts may still appear.

Google's testing-mode seven-day refresh-token expiry excludes apps requesting
only `openid email profile`; do not add extra scopes or publish the OAuth app
just to address forced consent. See [Google's token rules](https://developers.google.com/identity/protocols/oauth2/)
and [prompt behavior](https://developers.google.com/identity/openid-connect/openid-connect).

Infuser owns the invitation list. Configure OAuth2 Proxy with `email_domains=["*"]`
so it validates Google identities, then Infuser checks each request against its
own enabled members. An uninvited Google account may authenticate with Google
but cannot see games, launch, publish, or read platform data. Do not also retain
an old `authenticated_emails_file`, or new Infuser invitations will fail upstream.
Use a static upstream (`static://202`) because Caddy routes `/oauth2/*` separately
and the platform calls `/oauth2/auth` directly to validate sessions.

Bootstrap an admin using the local CLI, as the service user so database ownership
and SQLite WAL files remain consistent:

```sh
sudo -u infuser /usr/local/bin/infuser member /etc/infuser/infuser.toml admin@example.com 'Admin' --admin
```

The same command can add members or apply `--disable`. Admins can subsequently
invite/disable members in the Library's admin tools. Use a person's intended display name;
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

Use `MemoryMax=512M` for the main daemon with publishing enabled. Publisher work
has two admission slots; game files stream to disk with a 64 MiB/file limit.
Keep `TasksMax=128`. Installing a new binary does not update an existing service
unit: apply unit changes separately and run `systemctl daemon-reload`.
Choose the game budget to leave
headroom for the daemon, shared services, and operating system.
The helper only needs 64 MiB. Publishing content has an aggregate disk budget,
independent of the game's world volumes and Docker images.

## Data and updates

Use [backup and recovery](backup-operations.md) for a consistent snapshot of
platform state, configuration and owned game volumes. A copy of the live SQLite
file alone is not a backup of the platform or its game worlds.

Once publishing-managed servers exist, do not roll back to pre-platform binaries that
cannot read their catalog. Keep a pre-upgrade backup for such a manual rollback.
The current auto-updater still checks health and retains the previous binary.

Unpublished builds are retained intentionally so an operator can recover or
republish them. Remove unused builds through the API to reclaim disk. Guest-save
cleanup runs once a minute; personal saves survive unpublish and updates.
Admin-only preview means hidden from ordinary members, not a separate server.

To create a game pass without opening the website:

```sh
sudo -u infuser infuser publisher-pass /etc/infuser/infuser.toml my-game /var/lib/infuser/my-game.credentials.json
```

The CLI creates a mode-0600 file, never prints its token, and refuses to overwrite
an existing file. Transfer it privately to the game-making agent; revoke it in
the admin UI when finished. Normal users use Add a game instead.

For an Infuser-side art agent, create a separate [all-games metadata key](art-agent-operations.md).
It can read private art prompts and edit presentation across games, without
publishing builds or accessing player saves.

For a test launch without sharing a Google session, a local operator can
run `infuser preview-game CONFIG APP EXISTING_ADMIN_EMAIL PRIVATE_OUTPUT_FILE`.
This accepts hidden client-only or on-demand web games and creates a single-use,
two-minute launch link with a **fresh guest** save namespace. It grants no portal,
publishing or admin access. Treat the output as private and close the test game
when done. Normal player launches still require Google and enabled membership.

## Multiple families

Use [private groups](private-groups.md) to invite a separate family. Ordinary
admin invites always belong to the inviting admin’s group. The local `member`
command adds new accounts to Home; it is not the way to invite another family.
For a manual publisher pass outside Home, append `--group GROUP_ID`.

## Chat notifications

The daemon sends standard VAPID-authenticated Web Push to the browser's push
service. It generates `web-push.key` in the private state directory automatically;
back it up with the database. No Firebase project, Apple developer account or
extra process is needed. The key survives updates. Replacing it requires devices
to turn notifications off and on again.


## Cloud save capacity

`controller.cloud_save_budget_mb` defaults to 512 MiB for checkpoint contents and
retry records. This is a logical storage budget, not a cap on the SQLite database
or its WAL. Monitor disk space and keep the configured free-disk reserve.

## Test launch links

For disposable hidden previews, the operator helper supports:
`infuser preview-game CONFIG APP ADMIN_EMAIL OUTPUT --join-session SESSION --join-player PLAYER`.
It retains the existing enabled-admin/hidden-game checks and writes a private,
single-use guest launch. No Google session is granted to the test agent.
