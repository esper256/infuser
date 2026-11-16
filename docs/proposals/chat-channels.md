# Chat channel foundations

Audience: Infuser developers.

Status: implemented internally. The existing group chat is still the only
website conversation. No new routes, channel picker, direct messages, or SDK
capabilities are exposed.

## Stable identity, unchanged behavior

A conversation has its own opaque, persistent channel ID. A private group has
one default channel, bound by a unique `default_group` relationship. Names are
presentation; neither a group rename nor a future conversation rename changes
message ownership or IDs. Empty groups acquire their channel on first use.

Existing `/api/chat` handlers resolve the selected group's default channel and
call the channel message operations. Responses, ETags, ordering, sender-wide
request IDs and rate limits, deletion rules, seven-day/100-row retention, drafts,
unread state, scrolling, and notifications keep their existing behavior. No web
assets change in this refactor.

## One policy for every consumer

`platform::channels::Channel::access` returns the visible message boundary and
write/moderation permissions, or denies access. Message reads, sends, retries,
deletions, notification enqueue, and notification delivery use this policy.
Membership, channel configuration, and visit grants are read in the same database
transaction as the operation. Notification delivery rechecks access when selecting
pending targets; notifications already handed to a push provider cannot be recalled.

Two audiences use the same message operations:

- A default group channel follows current group membership. The installation
  operator can inspect it without writing; a resident admin can moderate it.
- An explicit channel follows `chat_participants` grants. Each grant has a message
  boundary and optional expiration. A channel can also expire as a whole.
  Group membership and operator inspection do not imply access to this audience.

The second audience is tested internal infrastructure. Creation, invitations,
moderation roles, participant changes, per-channel notification preferences, and
UI are future product decisions. Direct messages can be explicit channels with
two participants; a custom chat can use more. No separate message engine is needed.

## Visits remain live access grants

A visit grants its selected participants access to the host's default channel.
It does not create a parallel conversation or copy membership into a channel.
Admission captures that channel's highest retained message ID in the same
transaction as the visit. Existing admission boundaries remain valid because
message IDs are not changed.

Residents retain the uninterrupted timeline. Visitors only see messages after
admission while the visit and their participation remain active. Returning for
another visit gives them a new boundary. Expiry or removal denies reads, writes,
deletions and retries, and cancels pending push targets. Visitor admins cannot
moderate the host's chat. Home chat remains accessible independently.

## Storage and migration

`chat_channels` stores identity, an optional default-group binding, and optional
expiration. `chat_participants` stores explicit grants. The physical message table
keeps its historical name, `library_chat`, and gains an indexed `channel_id`.
New message operations scope storage by channel, not private group.

The startup migration runs in one immediate SQLite transaction. It assigns one
channel per group with existing messages and fills the new column in place. It
does not copy, renumber, prune, or recreate messages. Authors, text, timestamps,
tombstones, retry IDs, SQLite's sequence high-water mark, visit boundaries,
subscriptions, and pending notification references survive. Reopening the store
reuses the same channel IDs. A failed migration rolls back and can be retried.

The existing `private_group` message column and index remain as a compatibility
bridge for the immediately preceding server version. A trigger maps inserts from
that version into the correct channel. New inserts must match their channel's
binding. This permits binary rollback for today's group chat without a second
message store or a destructive down-migration. Older versions still need to
support visits to serve a database containing visits.

Explicit-channel rows use a reserved non-group compatibility value. They are not
exposed by an older server's ordinary group endpoints. Before exposing new channel
features, revisit the supported rollback versions and enforce their access rules;
legacy binaries do not understand channel or participant expiration.

Push subscriptions belong to devices/users. Delivery follows the queued message's
channel, never the group's old subscription snapshot. The outbox continues to
coalesce to the latest message per device and sends the same content-free alert.
Message IDs already identify their channel, so a second routing field in the
outbox would add a source of inconsistency. Future channel-specific notifications
and destinations need their own product decision; this change does not add them.

## Evidence

`chat_channels_journey` upgrades populated stores from before private groups and
from before channels. It verifies IDs, sequence preservation, tombstones, retries,
active and expired visit windows, old queued notifications, old-writer inserts,
restart stability, and rollback after an injected migration failure. It also
exercises explicit participants and expiration through the same message and push
operations without adding website routes.

Existing chat, notifications, private-group, and visits journeys remain regression
coverage for the current API. Browser checks cover the unchanged chat UI and the
visit flow. The normal full Rust suite and Clippy run before release.
