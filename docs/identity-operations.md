# Identity migration and recovery

Audience: server operators.

The identity foundation gives each member a permanent account and profile.
Google sign-in, invitations, names, games and saves keep working as before.
The [profile chooser](profiles.md) supports accounts invited into multiple groups.
Additional sign-in providers are not enabled yet.

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
and is not supported yet.

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
