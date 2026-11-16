# Implementation and compatibility notes

Audience: Infuser developers.

The [documentation index](README.md) links to the
current user-facing contracts. These notes describe storage and implementation
constraints, not additional steps for game authors.

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

The Library HTML embeds a revision derived from the release build SHA and its
shared client assets. `GET /api/client-state` returns `{version, game_open}` for
an enabled member. Every thirty seconds, a visible Library compares revisions.
It reloads only when the server confirms no recent game launch for that member
(including guests, other tabs/devices and native launches), no dialog/menu or
launch/send is active, no chat draft exists, no editor has focus, and there has
been no interaction for five seconds. A game is recent while its launch is
unexpired and has activity within ninety seconds. After a game closes, its last
heartbeat may therefore defer a refresh for up to ninety seconds plus polling.

Offline/errors and malformed responses never trigger a reload. The initial
revision comes from the loaded HTML, not a potentially newer first poll. A page
opened before update detection shipped needs one manual refresh to acquire it.

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
