# Temporary visits between private groups

Audience: Infuser developers.

Status: product decisions accepted; implementation pending. Account for visits
before freezing SDK identity, deployment, save ownership, and access lifetime.
The platform currently supports permanent home-group membership only.

## Participants and ownership

Only the installation operator can arrange visits. Regular group admins cannot
create them. The operator selects the host group, individual visitors, brought
games, and time window. Unselected members gain no host-group access. A visit can
end early for one participant or everyone; extending access is explicit.

Visitors keep their home membership. They can play the host's available games
together with residents. Residents can play the selected brought games. This
does not reveal the visitors' home library, chat, players, or server worlds, or
grant publishing or administration authority over the host group.

A game created by a visitor during an active visit belongs to the author's home
group immediately and is automatically included in the visit. Publication uses
home-group ownership and credentials. Ending the visit removes host access; no
ownership transfer or republishing is required. Availability still requires a
playable published build. Agents should not have to manage visits.

## Fresh once, always resume afterward

Each brought game has one persistent deployment in a particular host group,
identified by **host group + source game ID**. Its first visit starts with fresh
world data and fresh personal saves. It never copies or merges home progress.

Later visits bringing that game to that host group always reopen the same
deployment and retained progress. There is no visit-specific Start fresh option,
fork, or save branch. Participant lists, event names, dates, titles, and builds
do not change this identity. New players start with empty personal saves;
returning players retain their own progress.

Example: Game A visits Group B on Saturday. Next month, a different subset brings
Game A to B. The same world and returning players' saves resume. Bringing Game A
to Group C uses a different deployment, initially empty. Members staying home
continue using the independent home deployment.

Host-owned games retain their ordinary home deployment. Visitors joining do not
reset residents' worlds or saves. Visitor personal progress there stays scoped
to that deployment and can resume on a later authorized visit.

After a visit, borrowed-game data is dormant: retained progress grants no access
and keeps no server running. Reopening restores persisted state, not an old
process, connection, session ID, or seamless reconnect. Guest mode remains
separate: fresh, temporary personal saves for each guest launch.

Never start two instances writing the same deployment's world. Visit grants do
not allocate runtime/storage identities. If overlapping visits are supported,
they must reference this same deployment and serialize its lifecycle. Ordinary
concurrent personal-save writes still need the existing conflict protocol.

## Resident chat history and visitor access

Keep one host-group chat timeline. Residents can read retained conversation before,
during, and after visits, including visitors' messages. Visitors can read only
messages created after their own admission and only while participation is active.
A return visit grants a new window, not earlier visit history.

Enforce this on history, pagination, live delivery, unread counts, and notifications.
Use server-established message boundaries; client timestamps and cursors cannot
widen access. Editing an old message must not make it newly visible. Expiry removes
access to previously readable history too; already delivered content cannot be recalled.

Show that visitors are present so residents understand the audience for new messages.
Existing seven-day/size retention bounds still apply; this is not permanent chat
archiving. Evaluate notification access again at delivery time.

## Ending access

Warn participants before scheduled expiry. At expiry or removal:

- Visitors lose host-game launch, join, social, chat, and save access.
- Hosts lose borrowed-game access when its visit authorization ends.
- Remove visiting presence and terminate affected authorized connections.
- Stop accepting player commands, then allow bounded server checkpoint/shutdown
  work. Shutdown grace is not extra play time or renewed visitor permission.
- Residents playing their own host-group games may continue. Activity and source
  deployments in visitors' home groups remain unaffected.

This differs from ordinary website logout, which leaves ongoing play alone.
An open tab, renewable credential, native launch, or existing connection must
not extend authorization. Wi-Fi changes are not visit start/end signals.

Dedicated gateways can enforce connection expiry. Direct peer traffic does not
cross that gateway: adapters must stop accepting expired participants and use
bounded authorization renewal. TURN or source-IP allowlisting alone does not
establish per-player revocation. Do not advertise reliable visit cutoff for a
transport until it passes expiry tests, including backgrounded hosts and lost
coordinator connectivity. Define the maximum enforcement delay explicitly; do
not claim instantaneous cutoff for arbitrary peer or direct-port games.

## SDK and data model

Separate account identity, permanent membership, temporary participation, source
game identity, deployment, and launch authorization. Bind each launch to its
verified play context; switching website tabs cannot retarget a running game.

Publishing and metadata belong to the source game. Runtime state, personal-save
namespaces, persistent volumes, presence, admission, and active release selection
belong to the deployment. One process per deployment replaces the assumption that
one source game has only one process across the installation. Immutable build
artifacts can be reused; authorization and mutable state cannot be shared.

Preserve existing accounts, game IDs, URLs, saves, volumes, and builds as their
original home deployments. Do not reinterpret public `app_id` as a deployment ID.
Add verified deployment context where needed and preserve old clients and copied
server adapters. Sharing must not grant a publisher host-group administration.

Games handle gameplay as usual; Infuser supplies scoped identity, saves, peers,
and endpoints. Exact joins cannot cross deployments. Audit storage, container
identity, credentials, release drains, origins, native caches, and network
admission for deployment scope before enabling visits.

## Build order and acceptance

1. **Deployment foundation.** Specify concrete IDs and compatibility; map existing
   games to home deployments without changing save ownership. Enforce isolated
   mutable storage and one runtime writer per deployment.
2. **Visit authorization.** Implement operator-only creation, participant/game
   selection, admission, expiry, and early termination. Test selected versus
   unselected users, direct API requests, and concurrent browser tabs.
3. **Brought games.** Reuse builds, create isolated deployments once, and always
   resume them. Automatically include new visitor-authored games. Test home/host
   isolation, changed participants/titles/builds, and repeated visits without
   duplicate worlds or ownership changes.
4. **Chat and presence.** Test admission boundaries, old history, stale cursors,
   edits, later arrivals, expiry, return visits, notifications, and uninterrupted
   resident history.
5. **Transport and lifecycle.** Test dedicated, WebRTC, PeerJS, and supported
   native paths: expiry while connected/asleep/offline from control, bounded
   shutdown, resumed persistent state, and rejected post-visit joins. Keep paths
   unsupported until their adapters meet the same contract.

Before implementation, specify exact wire fields, expiry enforcement bounds for
each transport, and update selection for borrowed deployments. Existing matched
client/server activation and data-migration rules remain requirements. These are
engineering details to review, not a reason to reopen accepted product choices
or require game authors to implement visits.

See [SDK decisions](../sdk-1.0-decisions.md),
[stabilization plan](sdk-1.0-stabilization.md), and
[current private-group behavior](../private-groups.md).
