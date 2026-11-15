# Declared cloud saves: SDK 1.0 design

**Implemented for dogfooding; overall SDK 1.0 remains experimental.** Query
`capabilities.cloud_checkpoints.version` before adopting this API. The existing
`saves.load/save` slot API remains available with its original behavior; the new
checkpoint API is separate and does not silently migrate old slot data.

## Files the game understands, snapshots Infuser can protect

An agent declares logical save files and groups related files into a checkpoint.
For Three.js, these are serialized game data, not operating-system paths or
permission to scan a laptop's filesystem. Infuser supplies their contents at load
time and accepts a coherent replacement snapshot when the game checkpoints.

Manifest declaration in `infuser.game.json`:

```json
{
  "cloud_saves": {
    "groups": [
      {
        "id": "campaign",
        "label": "Adventure progress",
        "files": [
          { "path": "progress.json", "format": "json" },
          { "path": "inventory.json", "format": "json" }
        ]
      },
      {
        "id": "settings",
        "label": "Settings",
        "files": [{ "path": "preferences.json", "format": "json" }]
      }
    ]
  }
}
```

Group IDs and relative file paths are stable across builds. Reject absolute paths,
traversal, undeclared files, duplicate paths, and reserved platform names. Adding
an optional file is compatible; renaming a file requires an explicit game migration.
The first browser adapter supports JSON; binary files and native filesystem
adapters are later capabilities, not reasons to build more engines now.

Each group is atomic. A snapshot is the complete set of present files in that
group; omitted files are absent in that snapshot. An empty new group has no head
revision and no files. A game must deliberately construct a complete checkpoint,
rather than uploading only changed files and accidentally deleting the others.
Settings can be saved independently because they are a separate group.

## Browser workflow

```js
const checkpoint = await infuser.cloud.load('campaign');
restoreGame(checkpoint.files); // The game owns defaults and schema migration.

const result = await infuser.cloud.commit('campaign', {
  expectedRevision: checkpoint.revision,
  commitId: crypto.randomUUID(),
  files: {
    'progress.json': { schema: 1, level: 7 },
    'inventory.json': { schema: 1, items: ['lantern'] }
  },
  summary: 'Level 7'
});
```

Import `cloud` from `/_infuser/sdk.js`, or use `window.Infuser.cloud` after the SDK
loads. The SDK deals with scoped identity, authentication, transfer, and typed failures.
The game controls when its state is coherent and what "saved" means. It may only
show success after server acknowledgement. It supplies a bounded, optional human
summary for a conflict chooser; the platform treats that summary as plain text.
File contents include the game's own schema version; the server does not execute
migration scripts or guess how two inventories should be combined.

## Revisions, conflicts, and retries

Use opaque revision IDs, independently of game builds. A successful commit stores
an immutable snapshot and atomically moves the group's head if `expectedRevision`
still matches. The server supplies timestamps and originating build metadata.

A stale commit stores the incoming candidate without moving the head, then returns
a typed conflict containing current and candidate revision IDs. Both versions
survive a controller restart. The game can request the two snapshots and present
"This device" versus "Cloud" using their summaries and server times. It may offer
a game-specific merge; Infuser never performs a generic merge.

Resolution chooses a retained revision or submits an explicitly merged snapshot,
with the expected current head. It creates a new revision and records both parents;
it does not destroy the alternatives. If another device changed the head again,
resolution reports a fresh conflict instead of overwriting that change.

`commitId` makes retries idempotent within the authenticated player/app/group.
Repeating the same ID and payload returns the original result, including conflicts;
reusing it with another payload is an error. A timeout is not evidence that the
write failed, so the SDK retries with the same ID. Concurrent retries must not
create duplicate branches or move the head twice.

## Boundaries to preserve

- Derive player/app/guest identity from the session, never request-body IDs.
  A guest gets empty groups and temporary revisions, with no merge into the owner.
- Declare capabilities and negotiate storage limits; avoid baking installation
  limits into every game. Bound group size, files, retained history, unresolved
  conflicts, and aggregate installation usage. Advertise the effective limits.
- Never evict an unresolved conflict or current head to accept another write.
  If space is unavailable, reject without acknowledgement and keep the game's
  pending state intact. The game shows an unsaved state and offers retry.
- Do not promise offline durability in 1.0. Network failure or full storage must
  remain visible; the game must not report success or discard its pending data.
- Upgrading or rolling back a game does not rewrite cloud history. An older build
  must recognize unsupported save schemas and offer a compatible retained
  revision, or stop with a clear message. Never silently reset an unreadable save.
- Pre-1.0 Firefly slot data can be explicitly migrated or reset. Real games require
  compatible readers and tested migrations; a daemon binary rollback cannot undo
  a save-schema migration.

## Design review outcome and first implementation slice

Implement explicit JSON checkpoint groups before file watchers, background
filesystem mirroring, automatic merging, or binary/native adapters. This gives
Infuser enough information to preserve coherent saves while leaving game-specific
meaning with the game. Freeze the versioned wire format and error payloads alongside
the browser adapter, then keep an old-client journey against subsequent releases.

The first meaningful journey uses two clients: both load a checkpoint, A commits,
B conflicts, the daemon restarts, both candidates remain, B resolves explicitly,
and a repeated request does not create another revision. Include a two-file atomic
checkpoint, a fresh guest, a rejected undeclared path, a storage-limit failure,
and an older build encountering a newer schema. Firefly can exercise this before
any additional engine work begins.

## Wire format and outcomes

The experimental browser adapter is `/_infuser/cloud-v1.js`. Its wire routes are:

- `GET /_infuser/cloud/v1/groups/{group}`: current snapshot and unresolved conflicts.
- `POST /_infuser/cloud/v1/groups/{group}`: complete checkpoint or explicit resolution.
- `GET /_infuser/cloud/v1/groups/{group}/revisions/{revision}`: retained snapshot.
- `GET /_infuser/cloud/v1/groups/{group}/history`: retained revision metadata, newest first.

The SDK uses camelCase arguments; the wire uses `commit_id`, `expected_revision`,
`files`, `summary`, and optional `resolve_candidate`. Explicitly provide
`expected_revision: null` for an empty group. A snapshot contains `revision`,
`files`, `summary`, `build_id`, `created_at`, `parents`, and `resolves`.

A successful commit returns HTTP 200 with `{status:"saved", revision}`. A stored
conflict returns HTTP 409 with `{status:"conflict", current_revision,
candidate_revision}`. The SDK returns both as ordinary results. Other failures
throw `CloudSaveError` with `code` and `status`. A quota failure does not store a
candidate or receipt and must not be shown as saved.

Use `cloud.read(group, revision)` to inspect either version, then choose complete
files explicitly:

```js
const result = await cloud.resolve('campaign', {
  candidateRevision: conflict.candidate_revision,
  expectedRevision: latest.revision,
  commitId: crypto.randomUUID(),
  files: chosen.files,
  summary: chosen.summary
});
// Another change may have won during the choice. Handle result.status again.
```

Limits are advertised: up to eight groups, sixteen JSON files per group, 1 MiB
of JSON file contents per checkpoint, sixteen recent revisions as a retention
target, and eight unresolved conflicts per group. Pending candidates, their
comparison heads/bases, and the current head are protected. Older unprotected
history may be trimmed to fit the 16 MiB group limit or installation budget.
Missing retained revisions return `revision_unavailable`; list history before
offering rollback choices. No generic schema migration runs on the server.

The installation defaults to a 512 MiB logical checkpoint/receipt budget,
configurable as `controller.cloud_save_budget_mb`. Logical bytes include bounded
metadata allowances; this is not a cap on SQLite's physical file or WAL size.
Disk-reserve checks and transactional write failures protect against running
out of disk. Retry receipts remain for the namespace's lifetime and count toward
the installation budget, even after their snapshots are retired. An old receipt
replays the original outcome; it does not assert that its revision is still the
head or retained. The SDK retries transient failures with the same serialized
payload; caller retries must reuse both the commit ID and content.

Guest rows and receipts are removed transactionally when their launch expires.
New tables preserve checkpoint history and declarations across a rollback to an
older daemon; that daemon can still operate its legacy APIs but cannot serve the
new checkpoint endpoints. Restoring the newer daemon restores their availability.
