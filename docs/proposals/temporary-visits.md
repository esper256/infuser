# Temporary visits between private groups

Audience: Infuser developers.

Status: implemented as an opt-in preview; SDK 1.0 remains unfrozen. See
[operator instructions](../private-groups.md#visits) for enabling visits and the API.

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

## Implemented contract and limits

Home deployments retain their source app ID, existing saves and Docker volumes.
Borrowed deployments have persistent opaque `deployment-*` IDs in a separate
SQLite table, unique per `(source app, host group)`. Context adds `deployment_id`
and optional `visit: {expires_at, authorization_seconds: 30}`. Source `app_id`,
publication credentials and immutable artifact paths stay unchanged. Titles and
slugs are presentation, never save keys; rename friendliness takes priority over
preserving a particular URL spelling.

Each launch binds to a specific visit grant. A later visit cannot revive an old
launch. Each host has at most one active visit, and each visitor at most one.
Admission happens atomically at creation, using the current maximum chat message
ID as the boundary. Extending a visit keeps that boundary. Ending it requires a
new grant next time; removing the final visitor ends the whole visit.

Borrowed deployments follow the source's activated build. A running borrowed
server or peer room drains independently before switching; client and server
builds stay matched. Dormant deployments update without starting a process.
Build deletion protects borrowed deployments' active releases. Backups include
their records and world volumes through the existing full-state backup path.

Enforcement is layered:

| Path | Cutoff |
| --- | --- |
| Member, game, save, join and signaling APIs | Every request checks current authorization. |
| Managed WebSocket connections | Authorization checked every two seconds between frames, including while idle. Each socket write has a three-second timeout; slow writes can delay cutoff, bounded to about 11 seconds of transport waits while the controller is scheduled. |
| Borrowed process | Reconciler releases demand and requests stop on its next cycle (normally two seconds); existing bounded Docker stop grace permits checkpointing, not new player traffic. |
| Supplied JS WebRTC/PeerJS adapters | A 25-second coordinator lease fences sends and received data; stale responses cannot renew an expired lease. Both wall and monotonic clocks cover browser sleep. |
| Web player wrapper | Five-second heartbeat, 30-second authorization watchdog; expiry removes the game frame without exiting fullscreen. Suspended tabs enforce on resumption. |
| Updated Infuser Play | Five-second heartbeat; stop on denial or after 30 seconds without authorization, with a 500 ms process-check interval. Recheck after downloading, before launch. |

The Godot web adapter uses its existing wall/monotonic bridge and lease checks.
Arbitrary modified adapters and standalone direct connections cannot be remotely
revoked by Infuser. Legacy direct-port server games are blocked from visits.
Peer adapters do not provide DRM or erase downloaded content. Physical laptop
sleep and real Windows acceptance remain separate hardware checks; deterministic
sleep tests do not replace them.

Fast journeys cover isolated JSON slots/checkpoints, recurring visits, expiry,
restart persistence, selected visitors, ownership, chat windows and notification
delivery checks. Dedicated tests run two deployments concurrently, reject
cross-deployment admission, expire a live socket and preserve the home socket.
Browser tests cover operator creation, visit entry, independent tabs, mobile
return and ending a visit. Native tests gate old clients and exercise process
termination; peer tests cover independent rooms, expiry and stale packets.

See [SDK decisions](../sdk-1.0-decisions.md),
[stabilization plan](sdk-1.0-stabilization.md), and
[current private-group behavior](../private-groups.md).
