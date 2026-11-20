# Identity migration and recovery

Audience: server operators.

The identity foundation gives each member a permanent account and profile.
Google sign-in, invitations, names, games and saves keep working as before.
The [profile chooser](profiles.md) supports accounts invited into multiple groups.
Google remains the only public sign-in provider. A member link, below, is an
operator-minted sign-in for an invited assistant. It is not a new button or a
way to create members.

## Before the first genesis deployment

This is a one-time storage boundary. Stop the automatic update timer before
publishing or installing the release, and wait for any active update to finish.
Retain a coherent pre-migration backup. A database-only rehearsal copy is useful
for this change but does not replace a complete recovery backup.

Stage the verified release executable outside `current`, then run:

```sh
sudo /path/to/candidate/infuser check /etc/infuser/infuser.toml
sudo /path/to/candidate/infuser state-check /etc/infuser/infuser.toml
sudo /path/to/candidate/infuser state-rehearse /etc/infuser/infuser.toml
```

The rehearsal takes an online SQLite snapshot in a private temporary directory
on the same host, migrates that copy, and checks existing profile IDs, catalog
contents and durable side-table data. It reports only a profile count, removes
the copy on exit, and never migrates live state. It does not test Google itself
or game binaries. Allow temporary disk space at least as large as the database.

Install this verified executable as `/usr/local/libexec/infuser-updater` **before**
the first activation. The old stable updater does not know the new rollback
boundary. Start the normal updater service; it waits for idle admission and
migrates live state when the new controller opens its database. Verify the exact
build at `/healthz`, sign-in and an existing game/save, then resume the timer.

Use the same genesis-capable executable for separately installed backup workers
and operator commands that read platform state. Reinstalling only the daemon
does not update those stable copies.

## Sign-in and invitations

Keep OAuth2 Proxy configured for Google with `set_xauthrequest=true`. Infuser now
requires both `X-Auth-Request-User` and `X-Auth-Request-Email` from its successful
auth response. If an old proxy session lacks the subject, sign out and in again;
do not restore email-only authentication.

An existing invitation binds to its Google account on the next successful login.
After binding, changing that account's email does not create a new profile or
lose progress. Admin screens edit members within the selected group. Inviting
that email into another group creates an independent profile; it does not move
the original one. Operator member commands accept an original member handle or
an unambiguous contact email. For accounts with multiple profiles, use the admin
screen in the target group to avoid an ambiguous CLI lookup. Linking another
provider or moving a profile to a different account is not an invitation edit
and is not supported yet. A member link cannot be added to an account that
already has a Google login.

## Member links

An assistant joins the same way a person does. Invite it from the family's
admin screen with its email and display name. That creates a normal pending
invitation in that group. Do not use `infuser member` for this: that command
adds accounts to Home and is the bootstrap path, not a group invitation.

Only an operator on the server, over SSH, can mint a sign-in link. There is no
button and no password. Run the command as the `infuser` user so the database
files stay owned by the service. After an update, `/usr/local/bin/infuser`
follows `/opt/infuser/current` when bootstrap installed that symlink. Confirm
`infuser version` matches `/healthz` before minting. If the host still has a
separately copied binary, run `/opt/infuser/current/infuser` instead.

```sh
sudo -u infuser infuser member-link /etc/infuser/infuser.toml ASSISTANT_EMAIL
```

The command prints one line: an `https` URL. It is single-use and expires in
ten minutes. Minting another link for the same account deletes unused links.
The URL is not written to a file and Infuser does not log it. Do not paste it
into a ticket, chat, or anywhere a preview bot will open it. A browser
prefetch (`Sec-Purpose: prefetch` or `Purpose: preview`) does not consume it;
an ordinary GET does.

Open the URL in the assistant's browser. Infuser claims the pending invitation
with its own login issuer, sets an `__Host-infuser_member` cookie, and
redirects to `/library`. The cookie is `Secure`, `HttpOnly`, `SameSite=Lax`,
and `Path=/`, with no `Domain` attribute. The server stores only a SHA-256
hash of the session token. The session lasts two hours. That is long enough
to browse, launch a game, and look at a slow one, and much shorter than the
seven-day Google session. `SameSite=Lax` sends the cookie on same-site loads
of the Library, including when the assistant opens the site from a bookmark,
and does not send it on a cross-site form post. Write requests still check
`Origin`.

The assistant is then a normal member of the invited group: chat, presence,
and games use the same routes as everyone else. Game launches still exchange a
separate `__Host-infuser_game` cookie on the game host. Google cookies stay on
the main site.

Identify the member the same way as other operator member commands: the
original profile handle, or a contact email that names one account. An account
in several groups has one sign-in and the usual profile chooser. Pass the
email when it names that one account, or any of its profile handles. If the
email matches more than one account, the command stops and asks for a profile
handle. `--list` prints those handles and never prints a link or session token:

```sh
sudo -u infuser infuser member-link /etc/infuser/infuser.toml ASSISTANT_EMAIL --list
sudo -u infuser infuser member-link /etc/infuser/infuser.toml PROFILE_HANDLE --revoke
```

`--revoke` deletes that account's unused links and sessions immediately. The
Library's Sign out link only clears the Google cookie. Caddy sends `/oauth2/*`
to OAuth2 Proxy, so Sign out does not clear a member-link cookie. Revoke is
how an operator signs the assistant out.

Mint a link only for the assistant's own invitation. A link cannot sign in as
an account that already has a Google login, so it cannot become a person who
has signed in. If you mint one for a pending invitation and it is redeemed,
that invitation is claimed by the link and Google can no longer claim it.

### What the edge proxy already allows

The player template, [`deploy/Caddyfile.example`](../deploy/Caddyfile.example),
does not put OAuth2 Proxy in front of the portal. `/oauth2/*` goes to the
proxy. `/v1/*` and `/healthz` stay private. Every other apex path, including
`/member-link/*`, is proxied to Infuser, which checks the session itself.
Wildcard game hosts still answer `/_infuser/server/*` only after Infuser's
`/_infuser/proxy-auth` accepts the game cookie. No Caddy or OAuth2 Proxy
change is required, and no new path is reachable without a sign-in except the
redemption route. A request on any other host does not redeem the link.

`infuser render-caddy` is the older static-site template. It wraps the apex in
`forward_auth` to OAuth2 Proxy and is not the player routing. Do not install
that output over `sites-enabled/infuser.caddy`. If a host added `forward_auth`
on the apex anyway, exempt only the redemption route, in front of the
authenticated handle:

```
handle /member-link/* {
    reverse_proxy 127.0.0.1:7777 {
        header_up X-Infuser-Client-IP {remote_host}
    }
}
```

Do not add an OAuth2 Proxy `skip_auth_route` for the rest of the site.

### Rollback

Opening the new binary creates empty `member_links` and `member_sessions`
tables. It does not rewrite existing rows, and `identity_version` stays 1.
Google logins and pending invitations are unchanged until a link is redeemed.

The updater snapshots the database before it starts the new binary. A failed
activation, or `infuser rollback`, restores that snapshot and then starts the
previous binary. Links, sessions, and claims made after the snapshot are
discarded with it. The restored snapshot is still identity version 1, so the
previous binary's `state-check` passes. If the snapshot cannot be restored,
the updater leaves the new binary in place and does not start the older one.

Starting an older binary by hand against the new database, without restoring
the snapshot, is a different case. `state-check` still passes. That binary
ignores the new tables, so member cookies and unused links stop working. A
redeemed link is an ordinary login record, which the older binary keeps. The
invitation is no longer pending, so Google still cannot claim it. The older
binary has no member-link redeem path. Do not delete the new tables or
identity fields to make an old binary start. Restore the snapshot instead.

## Supported recovery

After successful migration, roll back only to genesis or newer compatible
releases. `state-check` is read-only and verifies the identity version before
activation or rollback. A failed activation restores the pre-update SQLite
snapshot first, then starts the previous binary. If that snapshot is missing or
cannot be restored, the updater keeps its recovery record and does not start an
older binary against state it cannot read. See
[Roll back a bad update](update-rollback.md). Deploy a compatible repair release
rather than deleting identity fields to make an old binary start.

Pre-genesis binaries also refuse the new catalog directly. Emergency recovery
to a pre-genesis version requires the matching complete pre-migration snapshot,
with the controller stopped, and discards changes made since that snapshot.
Routine operation and future rollback should not need that path.
