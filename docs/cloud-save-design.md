# Declared cloud checkpoints

Audience: game authors and their agents.

**Implemented preview; the overall SDK 1.0 contract is not frozen.** Query
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
Declare each file as `json` or `binary`. Browser and Godot web/native adapters
support both; they do not scan or automatically synchronize the filesystem.
Keep a path's format stable. To migrate JSON to binary, add a new path and ship a
reader that understands both before retiring the old file.

Each group is atomic. A snapshot is the complete set of present files in that
group; omitted files are absent in that snapshot. An empty new group has no head
revision and no files. A game must deliberately construct a complete checkpoint,
rather than uploading only changed files and accidentally deleting the others.
Settings can be saved independently because they are a separate group.

## Browser workflow

```js
import {ready, cloud} from '/_infuser/sdk/v1.js';
await ready;
const checkpoint = await cloud.load('campaign');
restoreGame(checkpoint.files); // The game owns defaults and schema migration.

const result = await cloud.commit('campaign', {
  expectedRevision: checkpoint.revision,
  commitId: crypto.randomUUID(),
  files: {
    'progress.json': { schema: 1, level: 7 },
    'inventory.json': { schema: 1, items: ['lantern'] }
  },
  summary: 'Level 7'
});
```

Import `cloud` from `/_infuser/sdk/v1.js`, or use `window.Infuser.cloud` after the SDK
loads. The SDK deals with scoped identity, authentication, transfer, and typed failures.
The game controls when its state is coherent and what "saved" means. It may only
show success after server acknowledgement. It supplies a bounded, optional human
summary for a conflict chooser; the platform treats that summary as plain text.
File contents include the game's own schema version; the server does not execute
migration scripts or guess how two inventories should be combined.

## Binary files

First check that `capabilities.cloud_checkpoints.formats` includes `binary`.
Declare, for example, `{"path":"world.bin","format":"binary"}`. Its wire value is
`{"base64":"AP8BgA=="}`: standard padded base64, with no other fields. Empty files
use an empty string. JSON files keep their original values, even if they happen
to contain a `base64` property.

```js
const context = await ready;
if (!context.capabilities.cloud_checkpoints.formats.includes('binary')) {
  throw Error('This Infuser needs an update before it can save this game.');
}
const loaded = await cloud.load('campaign');
const bytes = loaded.files['world.bin'] === undefined
  ? null : cloud.decodeBinary(loaded.files['world.bin']); // Uint8Array
// Restore bytes using your game's reader, then later:
const files = {...loaded.files, 'world.bin': cloud.encodeBinary(worldBytes)};
// Pass files to cloud.commit with loaded.revision and a fresh commitId.
```

Godot uses `Infuser.encode_binary(bytes: PackedByteArray)` and
`Infuser.decode_binary(value)`. Decoding returns `{ok:true,value:PackedByteArray}`
or `{ok:false,error}`. Read an existing `user://` file with FileAccess, encode it,
and include it in the complete checkpoint. On load, validate/decode every file
before replacing local progress. Keep local files on any error; the adapter does
not write them for you. The conflict chooser accepts mixed JSON/binary snapshots.

Snapshots add `formats`, mapping binary paths to `"binary"`; missing entries mean
JSON. Old JSON snapshots and commit bodies are unchanged. Writes based on a
snapshot whose file formats the build no longer declares are rejected.

The 1 MiB checkpoint limit measures the serialized `files` JSON, including paths,
JSON syntax and base64 overhead. A binary-only checkpoint therefore holds slightly
less than 768 KiB of raw bytes. History and installation quotas also count stored
encoded bytes. Binary files share the existing retries, conflicts, retention,
permissions and guest cleanup; they are not a separate storage system.

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

Infuser saves span builds. If an older build reads a save format it cannot
understand, it must refuse to overwrite it. Each web build has its own origin,
so a newly published build starts with fresh `localStorage` and IndexedDB.
Use Infuser saves for progress that should survive an update.

## Save handling requirements

- Use the identity provided by the SDK. A guest gets empty groups and temporary
  revisions; never merge these into the owner's progress.
- Declare required capabilities and read the advertised storage limits. Different
  installations may have different budgets; do not hardcode their limits.
- If storage is full, Infuser rejects the write without removing current saves
  or unresolved conflicts. Keep pending progress, show it as unsaved, and offer retry.
- Do not promise offline durability in 1.0. Network failure or full storage must
  remain visible; the game must not report success or discard its pending data.
- Upgrading or rolling back a game does not rewrite cloud history. An older build
  must recognize unsupported save schemas and offer a compatible retained
  revision, or stop with a clear message. Never silently reset an unreadable save.
- Published games require compatible readers and tested migrations. A daemon
  binary rollback cannot undo a save-schema migration.

## Wire format and outcomes

The experimental browser adapter is `/_infuser/sdk/v1/cloud.js`. Its wire routes are:

- `GET /_infuser/api/v1/cloud/v1/groups/{group}`: current snapshot and unresolved conflicts.
- `POST /_infuser/api/v1/cloud/v1/groups/{group}`: complete checkpoint or explicit resolution.
- `GET /_infuser/api/v1/cloud/v1/groups/{group}/revisions/{revision}`: retained snapshot.
- `GET /_infuser/api/v1/cloud/v1/groups/{group}/history`: retained revision metadata, newest first.

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

Limits are advertised: up to eight groups, sixteen files per group, 1 MiB
of serialized `files` JSON per checkpoint (including encoded binary files),
sixteen recent revisions as a retention target, and eight unresolved conflicts
per group. Current saves and versions needed to resolve conflicts are protected.
Older unprotected history may be trimmed to fit the 16 MiB group limit or
installation budget.
Missing retained revisions return `revision_unavailable`; list history before
offering rollback choices. No generic schema migration runs on the server.

Retry records count toward the installation budget even after their snapshots
are removed. Retrying an old commit returns its original outcome; that revision
may no longer be current or retained. Reuse both the commit ID and content when
retrying. Guest checkpoints are removed when their launch expires.
