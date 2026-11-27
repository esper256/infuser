# Implementation and compatibility notes

Audience: Infuser developers.

The [documentation index](README.md) links to the
current user-facing contracts. These notes describe storage and implementation
constraints, not additional steps for game authors.

All binary-rollback claims below apply only to identity-genesis or newer
compatible releases. The [identity foundation](proposals/identity-foundation.md)
preserves player IDs and storage handles but deliberately rejects pre-genesis
writers. It is the new minimum recovery baseline.

## Catalog and activity storage

`platform_catalog` holds cold catalog JSON. Launches and presence use small
`runtime_launches` and `runtime_presence` rows; heartbeats and Library reads do
not rewrite file inventories. Presence remains durable to keep rollback simple.
A cached catalog shares immutable file maps. SQLite revision triggers invalidate
it after catalog or side-table changes, including writes by a separate CLI.
Authorization still reads current activity and membership; there is no auth TTL.

Migration runs in one SQLite transaction. The old `platform` table becomes a
writable compatibility view that reconstructs the original JSON. Its triggers
translate old INSERT/UPDATE operations into the authoritative tables, preserving
renewals when a compatible older daemon or CLI writes. This supports binary rollback without
a two-release migration or a second stale catalog copy. It does not make older
binaries support newer SDK features. SQLite keeps `synchronous=FULL`.

## Publication and runtime recovery

Publisher mutations share two admission slots before reading request bodies and
serialize per app. Artifact bodies stream to private temporary files, with size,
time and disk limits. Cancellation removes staging files; startup removes abandoned
staging. Captures stream from the same file into SQLite blobs. Blocking validation,
hashing and file work retain admission until complete even if the caller disconnects.
Global commits still serialize disk-budget accounting and matched-build activation.
Bulk artifact writes and deletion do not hold the gameplay coordination lock.

The catalog's published app/build is authoritative. On startup and periodic
reconciliation, a controller registration written before an interrupted catalog
commit is restored to the catalog's version. A mismatched running attempt stops
before replacement. A missing publication means retirement: stop, confirm cleanup,
then unregister and reclaim its port. Retained artifacts, saves and world volumes
are untouched; operator-configured games are never retired this way. Pending
promotions remain durable and retry after the current server sleeps.

The lifecycle loop runs at most two independent engine operations, one per app,
with round-robin scheduling. Completion requires the same attempt and phase;
a stop request wins over a late start. Ready upstream addresses are verified once
per lifecycle observation and cached for requests. Stop/replacement invalidates
them; restart rebuilds them. Publication and minute-based expired-launch/save
cleanup run separately, so one slow Docker operation cannot hold up either loop.

## Precompressed web files

Publish writes `.br` and `.gz` siblings for large `.wasm`, JavaScript, text, and
`.pck` files before the build becomes current. Startup runs that same pass
after the API is already listening, on one background thread, one file at a
time. `/healthz` does not wait for it. The thread lowers its own scheduling
priority and is not a Tokio worker, so accepting health checks stays on the
async runtime. A repeat pass keeps an existing sibling when the source length
and modification time are unchanged. A sibling left on disk without that record,
which is what a start killed during the pass leaves behind, is kept when it
decodes to the source instead of being compressed again. The record lives
under `precompress/` in the state directory, outside the files players can
request. Siblings are not catalog entries, so they do not count toward the
content budget or the 512 MiB build limit, and a direct request for the
sibling name is a 404.

A sibling is stored only when the original is between 8 KiB and 64 MiB inclusive and the copy saves
at least 4 KiB and 10 percent. Images, audio, archives, `.unityweb`, and an
uploaded `.br` or `.gz` are left alone. Brotli quality is 5 with a 4 MiB window,
and gzip is level 6, so one engine module finishes well inside the publisher
client's four-minute timeout and the service memory limit. Writing a copy that
would cross the free-disk reserve is skipped; the original is still served.
That skip is not remembered, so a later start tries again.

Players that send `Accept-Encoding` receive the matching sibling with that
encoding, the original file's content type, and `Vary: accept-encoding`.
Cache-Control, referrer policy, nosniff, and COOP/COEP are unchanged. A client
that does not accept compression receives the original file. Byte ranges are
also served from the original file: a range of a compressed sibling is not a
range of the resource. Godot 4.7's web loader uses `fetch`, which decompresses
before the engine sees the bytes, and counts those bytes against the
uncompressed sizes stored in the export.

## Native run storage

Infuser Play holds its per-game install lock through verification, run preparation
and pruning. Warm launches verify once. Linux uses copy-on-write file clones where
available, with ordinary copies as the fallback; writable hardlinks are not used.
Current and previous cache versions, plus versions needed by active runs, survive
pruning. Run markers identify the companion and game by PID and process start
identity, so parent death or PID reuse alone does not justify deleting files.

Known-dead runs and interrupted staging directories are reclaimed on later launches.
Legacy runs without markers, unreadable liveness, and a crash during the uncertain
spawn window are retained conservatively. These exceptions can require manual
cleanup; this is not an absolute disk quota. Personal and guest saves are outside
this pruning policy. Actual Windows installation/play acceptance remains manual.

## SDK route compatibility

Major 1 modules live under `sdk/v1/`; their HTTP route adapter is
`src/portal/sdk_v1.rs`. Legacy imports and wire routes remain served separately.
They share handlers where behavior matches today. A future incompatible major
must add a new adapter and split changed handlers, never redirect old callers
to the new behavior. Compatible fixes can update a major's files, so responses
revalidate rather than claiming immutable JavaScript bytes.

## SDK declaration storage

Declarations are stored in an additive SQLite side table so an older catalog
writer cannot erase them. This does not make a daemon predating versioned routes
capable of serving major 1. Roll back to a release supporting every deployed SDK
major; older games retain their unversioned path. No save schema is migrated by
opting a game into this SDK major.

## Launch intent storage

Manifest opt-in is stored beside the legacy catalog so older daemons can read and
rewrite that catalog without losing the declaration. Pending intents are short-
lived launch metadata: restart on the same version preserves them, while rollback
to an older binary may discard them. Falling back to a title screen is intentional;
there is no promise to replay an intent across a downgrade. Saves are unaffected.

## Runtime declaration storage

The declaration lives beside the legacy catalog, so an older daemon can still
read/rewrite that catalog without deleting it. Rolling back to a daemon without
runtime v1 restores its old warm-on-launch behavior; a new game's capability
check must show that this feature requires a newer Infuser. Binary rollback does
not provide missing SDK methods. Upgrading again restores the declaration.

## Slug storage

Slug and alias records live outside legacy catalog JSON so older catalog writers
cannot erase them. A pre-feature daemon will not resolve the new links; restoring
a supported daemon restores the same assignments without reallocating them.
Deleting a hidden game removes that game's `game_urls` row. A pre-update rollback
snapshot can still contain it; see [roll back a bad update](update-rollback.md).

## Admin removal and locked titles

`title_locked` is a column on `game_metadata`. Publisher title writes refuse to
change a locked name. Promotion does not copy the manifest title into that column.
Hidden-game deletion removes the catalog entry, side-table rows for that app and
its deployment ids, saves, cloud checkpoints, screenshots, publish captures, web
files, server keys, and packaged containers, volumes, and images. It refuses while
a peer room, reserved dedicated server, or publish/packaging job is active, and
does not stop them. The append-only `admin-audit.jsonl` in the state directory
records the actor, app, title, and time. Chat text stays; a chat row that pointed
at a deleted screenshot keeps the text and drops the image reference.

## Managed packaging internals

Rust validates lockfile package URLs and integrity hashes, then fetches bounded
tarballs from the approved npm registry without redirects. Package installation
runs offline in one non-root container with no Docker socket, no platform secrets,
no game volumes, a read-only root filesystem, bounded temporary storage, CPU/RAM
limits and a deadline. npm lifecycle scripts are disabled. Repository npm config,
Git/URL/local dependencies and workspace links are rejected in this first recipe.

Image assembly copies the bounded prepared files into an unstarted container and
commits its filesystem with platform-selected settings. It does not invoke Docker
build or create a build cache. No project Dockerfile or build command runs.
A separate disposable container checks readiness
without network access or live game data. The resulting local image ID, base image,
lockfile hash and recipe version are recorded with the build.

Managed servers use a separate internal Docker network. There is no Internet
egress or route to cloud metadata; the installation hostname resolves to the
bridge's host gateway so the server can reach Infuser's HTTPS API. The controller's
private API remains bound to host loopback. These containers publish no host port;
after checking the player's lease, the controller gives Caddy the owned container's
bridge address. Caddy must run in the host network. This recipe does not support external
backend services. Existing operator-staged images retain their declared network
policy. The packaging worker and readiness probe have no networking at all.

One worker runs at a time. Its reservation participates in the controller's host
resource accounting. A restart cleans up owned packaging containers before releasing
that reservation. Disk reserve and image retention limits apply; active and retained
build images are not pruned to make a new build succeed.

## Publication replay

A future replay tool should consume the [publication capture format](publication-capture.md), require fresh test-only
credentials, remap app/build identifiers as needed, and run against a disposable
controller and storage namespace. It must report missing predecessors, skipped
requests and unknown outcomes. Prefer importer/validation comparisons before any
runtime execution; archived JavaScript, native files and server inputs are still
untrusted code. Production must never be the implicit replay target.

The fast capture journey already reconstructs a published game and update from
exported bodies against a separate local controller with new credentials, checks
that rejected JSON is preserved byte-for-byte, and verifies redaction, deduplication,
budget gaps and unknown outcomes.

## Library presentation and refresh

Chat uses one room per private group. Preserve these UI behaviors when changing
the implementation:

- The panel targets 437 × 672 CSS pixels, clamped to the viewport and phone keyboard.
- Pointers connect recent speakers to visible online portraits: the last twelve
  messages and five minutes of activity. They follow wrapping and theme changes.
- Hiding chat preserves the draft. Failed sends retain the original request ID.
- Reading history never jumps to the bottom because of polling, unread counts,
  presence changes, or a small upward scroll.
- The unread badge counts messages received during this page session, not initial
  history. Closed chat briefly wiggles every eight seconds unless motion is reduced.

Storage retains at most 100 records including deletion markers. Reads show the
last seven days; sending prunes older entries. Removed text is cleared immediately.
Disabled members' messages are hidden; re-enabling restores retained messages.
Chat uses a separate SQLite table so legacy catalog writes cannot erase it.

The UI polls every three seconds when open and ten when closed, pauses in hidden
tabs, and times requests out after twelve seconds. Reconnects do not repeatedly
show toasts. Player instructions live in [chat help](library-chat.md); endpoint
contracts live in the [chat API](chat-api.md).

## Automatic client updates

The Library and web player HTML carry the Infuser client version and the current
emergency reload generation. The version incorporates the release build SHA and
shared assets. The Library gets `{version, force_reload, game_open}` from
`GET /api/client-state` on its existing 30-second timer, on becoming visible,
and on reconnect. The browser may suspend background tabs.

For ordinary updates the Library waits until visible and idle for five seconds.
A local voice connection (including connecting and reconnecting), an open
dialog or menu, a draft or focused editor, a launch or send in progress, and
a recent game launch each defer the reload. A launch is recent while unexpired
and seen within 90 seconds, including guest and native launches for the profile.
Voice in another tab can continue while this Library refreshes.

The game heartbeat carries the same version and generation in `client_update`.
The SDK forwards changes to its enclosing player. Visits also receive them on
their existing heartbeat. An ordinary mismatch shows a message in the player
header. Visit warnings and ended-session messages take priority. A separate
`update_available` flag describes the published game build.

`force_reload` is a monotonic integer. Zero is the shipped default. A future
emergency release can raise the generation to reload older open pages at their
next successful check, even during voice, gameplay, editing, or fullscreen.
Keep the generation in later releases. Both pages compare it to the generation
in their loaded HTML, so a fresh page acknowledges the command even if the
server keeps sending it. Older clients first need to load a release that
understands the directive.

## Push notification delivery

Subscriptions and a coalescing outbox live in separate SQLite tables. Message
retries don't re-notify. Delivery rechecks group membership, enabled accounts and
message removal; dead endpoints are removed. Temporary failures retry up to three
times, and queued work expires after five minutes. Provider-side delivery expires
after one minute, avoiding a burst of stale chat alerts when a phone reconnects.
Browser subscriptions unused for 180 days are removed.

## Feedback storage and retries

### Storage, limits and retention

The private `feedback-queue.json` file uses Infuser's existing atomic, mode-0600
file replacement. It stores the group, hashed submitter ID, random request/entry
identifiers, report and state. The list API does not expose submitter identifiers.
Pending and uncertain reports expire 30 days after intake. Successful posting or
dismissal immediately deletes the report text and diagnostic snapshot. Small
hashed deduplication tombstones remain until the same 30-day deadline. Cleanup
runs during normal controller maintenance (about every minute) and on queue
access. Stopping the controller pauses cleanup; backups require their own
retention policy. Corrupt storage fails closed for feedback and never triggers a
public submission. Retention errors are logged without report content.

Intake allows 3 new reports per member per rolling day, 50 per installation per
day, and 500 retained entries including tombstones. Requests are capped at 16 KiB;
titles at 80 characters and descriptions at 2000. Unknown fields are rejected.
Public posting allows one attempt per group per minute and ten per installation
per rolling day. Failed upstream attempts consume that budget. Limits persist
across restarts. Run one controller per state directory, as with normal Infuser.

### Retries and uncertain outcomes

The client retries private intake with the same request ID and frozen draft;
replays return the original entry. Identical drafts from the same member/group
are also deduplicated for the retention window, including dismissed reports (up to 32 retry IDs per identical draft).
Reusing a request ID with different content returns 409. Unsent client drafts
are only in page memory; closing/reloading the form discards them.

Before contacting GitHub, the server durably records the approved version and
marks the entry as attempted. Only one caller can claim a pending entry. A
successful retry of a confirmed submission returns success without another POST,
provided the approved content matches. GitHub errors, lost responses or a crash
leave an uncertain/in-progress entry that cannot be posted again, even after a
restart. Admins should check GitHub, then dismiss it; if absent, deliberately
create a revised report. There is no automatic public retry or reconciliation.
An in-flight entry cannot be dismissed for its first minute. This conservative
at-most-once policy favors avoiding duplicate public issues over automatic recovery.

## Infuser Play updater

The installed executable is a stable launcher. Windows protocol registration and
shortcuts keep pointing to it; the Flatpak executable remains in its read-only
installation. Updated executables live in `play-updates` inside Infuser Play's
application data directory. Updating does not change account connections, personal
saves, guest namespaces, or running games. An already running companion finishes
its session with its original version.

Only this project's published GitHub releases supply updates. Game manifests and
Infuser servers cannot choose the companion executable. `play-release.json` names
a full commit and SHA-256 digest for each platform. Downloads use that commit's
release URL, have bounded size/time, and must match the digest before an atomic
install. HTTPS protects the manifest; checksums are not independent code signatures.
The Windows preview remains unsigned.

A new worker must create its window and acknowledge readiness before the launcher
closes its window. The launcher then waits without a window until the worker exits,
keeping Flatpak’s sandbox alive. A failed startup falls back to the previous verified worker, then the
bundled copy. The failed release is skipped until another release appears.
The active and previous workers are retained. A per-user lock prevents simultaneous
launches from racing update activation. No update interrupts an existing game.
This handshake checks startup, not every game operation; release tests still matter.

Development builds without `INFUSER_BUILD_SHA` do not download updates. Forks should
change the release repository in `desktop/src/update.rs` when packaging their own
companion. Existing preview installations from before this feature need the new
installer once; later releases update themselves.

## Checkpoint storage and quotas

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

## Screenshots

The website uses `getDisplayMedia` and Region Capture to capture the game viewport
without asking the game to read its canvas. A successful crop is required before
saving; choosing another tab/window cannot silently upload the desktop. Capture
is best-effort and needs browser permission each time. Audio is never requested.

`POST /_infuser/screenshots` uses the authenticated game launch and exact-origin
check. WebP originals retain capture resolution up to 4K (4096 pixels per side,
8,294,400 pixels total and 6 MiB); thumbnails are at most 320 pixels per side
and 96 KiB. Image decoding is serialized and validates both files. The private
`screenshots` SQLite table keeps the latest ten captures per profile plus any older
images still attached to reactions. Gallery insertion and oldest-image eviction share a transaction. These rows are included
in the existing SQLite backup. Physical database space is reused after eviction.

The website's `/api/screenshots` lists only the selected profile's collection.
Image reads return private, non-cacheable data through the pinned profile API.
An attachment is a screenshot ID, never a public URL or a copied image. Other
profiles must provide a live, visible chat-message or reaction context. Chat
reads apply channel membership, visit admission boundaries, deletion and history
retention. Reactions use the visible deployment. Chat attachments expire when a
capture leaves the gallery. A reaction keeps the original and thumbnail until
its last reference is removed.
