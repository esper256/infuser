# Identity foundation: genesis

Audience: Infuser developers.

Status: implemented foundation. Google remains the only sign-in provider and the
website still opens one profile. Profile selection and additional providers are
separate follow-up work.

## Permanent decisions

An account represents authentication and account-wide authority. A profile
represents a person in one private group. A provider identity signs in to an
account; it does not name a profile or own game progress.

| Record | Identity and responsibility |
| --- | --- |
| Account | Random permanent ID; enabled flag; operator authority |
| Provider identity | Unique `(issuer, subject)` linked to one account; contact email is mutable |
| Profile | Permanent player ID; belongs to one account and one group; owns its name, avatar, games, saves and group-admin role |
| Session or credential | Acts through one profile; never grants access to all profiles on an account |

There can be one profile per account per group. Group admin does not carry across
profiles. The operator role belongs to the account, but inspection remains explicit
and does not make the operator an ordinary participant in another group.

Games receive the selected profile's opaque `player_id`, never an account ID or
provider identity. Existing player IDs stay byte-for-byte identical. Guest IDs
and visit save isolation keep their existing semantics. A visit is temporary
access by an existing profile, not a second profile or an account link.

## Authentication boundary

The configured Google OAuth2 Proxy authenticates the browser. Infuser reads its
successful response's `X-Auth-Request-User` as Google's subject and
`X-Auth-Request-Email` as the verified email. Incoming browser identity headers
are discarded. Missing subject fails authentication; there is no email fallback.

An existing invitation can be claimed once by that verified email. Claiming it
persists the Google issuer and subject and removes the pending invitation. Later
email changes update contact information only. A different subject cannot claim
the same profile by presenting its old or new email. This follows the stable-ID
rules in [Google's OIDC guide](https://developers.google.com/identity/openid-connect/openid-connect)
and [OpenID Connect](https://openid.net/specs/openid-connect-core-1_0.html).

The data model accepts other issuers. Adding a provider still requires an explicit
trusted adapter and an authenticated account-linking flow. Matching email
addresses across providers must never silently link accounts.

## Migration without rekeying saves

The catalog stores `identity_version: 1`, accounts, profile mappings and provider
links. Each existing member gets an account and a stored profile ID equal to its
previous player ID. Existing operator entries become account IDs. These changes
commit together in SQLite; a failed transaction preserves the prior catalog.

Historical member keys look like email addresses. They are now immutable storage
handles. Existing fields named `email` in launches, native devices, chat, visits
and ownership records retain those handles. Do not replace them with a provider's
current email. `Data.player_id()` returns the stored identity; runtime code must
not hash a login email to identify a player.

Migration and the current invitation flow initialize a first profile's ID from
the historical handle once. A future second-profile creation flow must generate
an independent opaque handle and permanent ID. It must not reuse email-derived
IDs. Removing or renaming a profile must not recycle its identity.

No game reupload, save copy, native re-pairing or publisher-key replacement is
required. Credentials keep their original profile scope. Profile names and
avatars do not change when a provider changes its contact details.

## Rollback floor

The catalog field formerly named `members` is written as `profiles`. Genesis can
read the old field during migration; pre-genesis binaries require it and refuse
the migrated catalog. This deliberate format boundary prevents an old writer
from discarding new identity links.

`state-check` rejects unsupported identity versions without writing. The stable
updater runs it before activation and before either immediate or interrupted
rollback. An incompatible rollback keeps the current selection and recovery
record rather than changing the symlink to an unusable daemon.

Deploy genesis with a separately upgraded stable updater and a retained snapshot.
After acceptance, genesis is the oldest supported rollback target. Returning
earlier would require restoring an entire matching pre-migration snapshot, losing
subsequent changes. See [operator procedure](../identity-operations.md).

## Next: profile selection

Authentication already resolves an account separately from profile selection.
Today one enabled profile is selected automatically; more than one returns an
explicit selection-required error. Do not add a second live profile before the
chooser is implemented.

The chooser will create a server-validated session bound to one selected profile.
Switch Profile will return to that chooser and reload the site. Old tabs, game
tickets, publisher keys and native credentials must remain bound to their original
profile; switching must not retarget them. Group permissions and visit windows
must be checked against that profile on every request. Browser storage, push
subscriptions and native login handoff need explicit profile scoping before
shipping the chooser.

## Evidence

`identity_journey` exercises invitation claiming, changed Google email, rejected
identity substitution, missing/spoofed headers, preserved saves and open game
sessions, separate profile IDs and group authority, and unsupported state versions.
`activity_storage` can run an actual pre-genesis executable to prove it cannot
write migrated state. `update_journey` covers failed activation and interrupted
recovery at the compatibility boundary. Existing visits, chat, publishing, native
and save journeys remain part of the required regression suite.
