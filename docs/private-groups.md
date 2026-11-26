# Private groups

Audience: server operators.

One Infuser installation can host separate families. An account can hold one
profile per private group, each with its own handle, picture, appearance, games,
saves, chat, reactions, stars, and voice volumes. Members see only their active
group’s games, players, chat, reactions, and tags. A family’s admins manage only
its invitations, game visibility, author credits, and agent connections. They
cannot browse or administer other groups.

Existing accounts, games, saves, and publishing connections belong to **Home**.
No republishing or credential replacement is needed. Inviting an email already
used in another group creates a separate profile in the new group (see
[profiles](profiles.md)); temporary visits can still share selected games
between groups without creating permanent membership.

## Operator setup

Deploy a version with private-group support first. In the private host config:

```toml
[controller]
private_groups = true
```

Restart Infuser, then grant the operator role to an existing enabled account:

```sh
sudo -u infuser infuser operator /etc/infuser/infuser.toml admin@example.com
```

Append `--revoke` to remove the role. This local command is the only way to grant
operator access. Being a family admin does not grant it. The role does not change
the account’s home group or its ordinary player behavior.

Operators open their profile picture menu, then **Admin tools → Groups**. This entry is hidden from other players,
including family admins without the operator role. During read-only inspection,
the dropdown contains only Groups; family-management controls stay hidden.
**Rename** changes the display name of any group, including the initial **Home** group. Members, games, and chat stay with the same group ID.
**Add a group** creates an empty group and invites its first admin by Google email. That admin
signs in normally and invites their family through Admin tools. Infuser does not
send invitation emails. If Google OAuth is still in testing mode, add their
accounts to its test-user list too.

An email already used in another group gets a separate profile in the new group.
Existing games, progress and permissions stay with the original profile. See
[profiles](profiles.md) for the chooser and Switch Profile behavior.

**View** opens a read-only inspection in that browser tab. The operator can see
the selected group’s Library, activity, and chat, including hidden previews.
Inspection does not announce the operator online, create a membership, launch
games, post messages, or modify anything. **Return home** restores normal use;
the tab also returns home after 15 minutes. Existing home-group games and other
tabs continue normally.

## Game author credits

Admins open **profile picture → Admin tools → Game authors**, choose an uploaded
game (including hidden games), and add or remove profiles from their library.
The arrow buttons set the credit order; **Save authors** applies it. At least one
author is required. Cards show overlapping portraits, while the full game popup
shows each portrait and name in order. Searching by any credited author finds
the game.

Credits are display metadata only. They do not change the original owner,
publishing tokens, tag-editing permissions, or update access. Games without
explicit credits use their original owner. Saved credits survive game updates.
The Library API returns the ordered `authors` array and keeps `author` as its
first entry for older clients. Admin endpoints are `GET /api/admin/game-authors`
and `PUT /api/admin/apps/{app}/authors` with an `authors` array of profile IDs.
Visitors cannot edit credits, and operator inspection remains read-only.

## Visits

After deploying a version with visit support, refresh the separate updater if
automatic server updates are installed:

```sh
sudo install -m 0755 /opt/infuser/current/infuser /usr/local/libexec/infuser-updater
```

The updater reads the controller configuration too; an older copy cannot parse
the new setting. Then add `visits = true` under the same `[controller]` section
and restart Infuser. Keep both flags enabled afterward;
older binaries must not serve a database with visits. Back up the full state
directory before upgrading. Do not remove the flags to bypass a rollback failure.

Open **profile menu → Visits**. Only the installation operator can start a visit:
select its host group, individual visitors, games to bring, and an end time within
seven days. All selected visitors are admitted immediately. One group can host
one active visit; a person can visit one group at a time. Selected visitors choose
**Visit** in their own profile menu. This choice affects only that browser tab.
**Return home** restores their normal Library. Hosts see brought games in their
ordinary Library automatically.

The operator can extend an active visit, end it, or remove individual visitors.
Removing the last visitor ends the visit. To return later, start another visit
with the desired people and games. Each brought game always resumes its previous
progress at that host group; its first visit starts fresh. The game's home
progress stays separate. Guest mode still starts with fresh temporary saves.

Visitors can play host games, see people online, react and chat. They cannot
administer the host group. Their newly created games remain home-owned and join
the visit automatically once published. When the visit ends, visitors lose host
access and hosts lose brought games. Saved visit progress is retained, and
borrowed servers shut down. Residents can continue playing their own games.

The Library banner identifies visitors and the end time. Visitors see chat only
from admission onward, and only during the visit. A later visit starts a new chat
window. Hosts retain visitor messages in their normal timeline, subject to the
usual seven-day/size retention.

Web games and managed WebSocket server games can participate. Legacy direct-port
servers need the managed adapter first; the Library explains this when play is
unavailable. Peer play requires the supplied Infuser adapters and their periodic
authorization checks. Updated **Infuser Play** supports visits for the existing
single-player native preview; older versions ask to update before claiming a
visit launch. Native multiplayer remains unsupported.

Visit API routes use an operator's Google session for management:

- `GET /api/operator/visits` returns groups, eligible players and games, and visits.
- `POST /api/operator/visits` takes `{name, host_group, participants, games,
  expires_at}`. Use player IDs and game IDs from that inventory; expiry is Unix
  seconds. The result contains the new visit `id`.
- `PUT /api/operator/visits/{id}` takes `{expires_at}` for an active visit.
- `DELETE /api/operator/visits/{id}` ends the visit.
- `DELETE /api/operator/visits/{id}/participants/{player_id}` removes a visitor.
- `GET /api/visits` returns the signed-in player's active accessible visits.
- `X-Infuser-Visit: VISIT_ID` selects one for member APIs. Game launches then carry
  their own verified context. The header never changes ownership or publishing
  authority, and cannot be combined with operator inspection.

Participant/game selections are fixed at creation, apart from removing visitors
and automatically including newly created games. Start a later visit to choose a
different selection; it reuses the same saved progress.

## API and agents

Normal requests infer the group from the authenticated member or publishing key.
A game manifest cannot select a group. The SDK stays unchanged. Game IDs are
unique across the installation; readable slugs and aliases are unique within a
group. Guessing an ID or copying another family’s link does not grant access.

Operator routes use the normal Google session and Origin checks:

- `GET /api/operator/groups` lists `{id,name,home}` records.
- `PUT /api/operator/groups/{id}` accepts `{"name":"Family Arcade"}` and returns the existing ID and new display name.
- `POST /api/operator/groups` takes `{name,admin_email,admin_name}` and returns
  `{id,name}`. An email already used in another group gets a separate profile here.
- `X-Infuser-Inspect-Group: GROUP_ID` selects the read-only group for an operator’s
  member API reads. The server checks the role on every request and rejects
  writes carrying this header. It never changes group membership. The 15-minute
  return is a browser convenience, not an expiring API credential.
- Cover URLs in inspection carry `?inspect_group=GROUP_ID` because image requests
  cannot set that header. The server still checks the signed-in operator role.

`infuser publisher-pass CONFIG APP OUTPUT --group GROUP_ID` creates a manual
publishing pass in that group. Omitting `--group` selects Home. Normal Add Game
prompts already do this correctly without additional choices for users or agents.

[Art-agent keys](art-agent-operations.md) default to Home; use `--group GROUP_ID` for another
group or explicit `--all-groups` for installation-wide artwork maintenance.
Existing all-games keys retain their access. Never give an installation-wide
key to a game-making agent. The controller’s host control token also remains an
installation-wide operator credential.

## Shared hosting and upgrades

The groups share one controller, database, web frontend, PeerServer, and TURN
service. Games retain their existing containers and idle shutdown behavior.
There are no new per-group CPU, RAM, or storage budgets. Private groups enforce
application access boundaries; they do not provide separate machines or separate
network firewalls. Dedicated game servers must still validate Infuser tickets.

Back up the complete state directory. Keep
`private_groups = true` once another group exists. Infuser refuses to serve a
multi-group database with it disabled. Older binaries reject the new config
field, preventing an automatic rollback from silently exposing other groups.
Do not remove this field to make an old binary start against multi-group data;
restore a group-aware version instead.
