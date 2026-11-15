# Private groups

One Infuser installation can host separate families. Each account belongs to one
private group. Members see that group’s games, players, chat, reactions, and tags.
A family’s adult admins manage only its invitations, game visibility, and agent
connections. They cannot browse or administer other groups.

Existing accounts, games, saves, and publishing connections belong to **Home**.
No republishing or credential replacement is needed. An account already assigned
to a group cannot be invited into another one. Moving accounts, sharing games
between groups, and multiple memberships are not implemented.

## Operator setup

Deploy a version with private-group support first. In the private host config:

```toml
[controller]
private_groups = true
```

Restart Infuser, then grant the operator role to an existing enabled account:

```sh
sudo -u infuser infuser operator /etc/infuser/infuser.toml adult@example.com
```

Append `--revoke` to remove the role. This local command is the only way to grant
operator access. Being a family admin does not grant it. The role does not change
the account’s home group or its ordinary player behavior.

Operators open **Admin tools → Groups**. This entry is hidden from other players,
including family admins without the operator role. During read-only inspection,
the dropdown contains only Groups; family-management controls stay hidden.
**Add a group** creates an empty group and invites its first adult by Google email. That adult
signs in normally and invites their family through Admin tools. Infuser does not
send invitation emails. If Google OAuth is still in testing mode, add their
accounts to its test-user list too.

**View** opens a read-only inspection in that browser tab. The operator can see
the selected group’s Library, activity, and chat, including hidden previews.
Inspection does not announce the operator online, create a membership, launch
games, post messages, or modify anything. **Return home** restores normal use;
the tab also returns home after 15 minutes. Existing home-group games and other
tabs continue normally.

## API and agents

Normal requests infer the group from the authenticated member or publishing key.
A game manifest cannot select a group. The SDK stays unchanged. Game IDs are
unique across the installation; readable slugs and aliases are unique within a
group. Guessing an ID or copying another family’s link does not grant access.

Operator routes use the normal Google session and Origin checks:

- `GET /api/operator/groups` lists `{id,name,home}` records.
- `POST /api/operator/groups` takes `{name,admin_email,admin_name}` and returns
  `{id,name}`. The email must not already belong to any group.
- `X-Infuser-Inspect-Group: GROUP_ID` selects the read-only group for an operator’s
  member API reads. The server checks the role on every request and rejects
  writes carrying this header. It never changes group membership. The 15-minute
  return is a browser convenience, not an expiring API credential.
- Cover URLs in inspection carry `?inspect_group=GROUP_ID` because image requests
  cannot set that header. The server still checks the signed-in operator role.

`infuser publisher-pass CONFIG APP OUTPUT --group GROUP_ID` creates a manual
publishing pass in that group. Omitting `--group` selects Home. Normal Add Game
prompts already do this correctly without additional choices for kids or agents.

[Art-agent keys](art-agent.md) default to Home; use `--group GROUP_ID` for another
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

Back up the state directory with the usual deployment backup. Group policy lives
in SQLite alongside the catalog; legacy chat rows belong to Home. Keep
`private_groups = true` once another group exists. Infuser refuses to serve a
multi-group database with it disabled. Older binaries reject the new config
field, preventing an automatic rollback from silently exposing other groups.
Do not remove this field to make an old binary start against multi-group data;
restore a group-aware version instead.
