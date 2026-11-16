# Architectural review: improvement plan

Audience: Infuser developers.

Status: core improvements implemented on 2026-11-16; evidence and remaining
limits are recorded below. The original assessment was against commit `b5154b9` on
2026-11-15. This document retains the findings, source checks, and acceptance plan
needed to evaluate the work; the original review's section numbers are historical
references. Findings were checked against source, not production load measurements.
Latency, memory, and effort estimates are hypotheses, not benchmark results.

## Implementation results

PR #34 addressed frontend polling and DOM churn, including hidden admin
diagnostics. The backend work here separately implements batches 1–5, plus
authorized ETags for Library/chat/art and cheaper packaging completion polling.
The numbered sections below retain the original acceptance plan; this record
distinguishes implemented choices from goals that still need measurement.

- **Activity:** small durable launch/presence rows and a revision-invalidated
  catalog cache replace routine whole-catalog rewrites. Presence stays durable,
  rather than moving to memory. A writable legacy SQL view supports old CLI and
  daemon access, so no preparatory compatibility release is required. The actual
  `b5154b9` CLI passed old → new writes → old writes → reopen tests. Saves retain
  full SQLite durability; no authorization cache was introduced.
- **Publishing:** two shared admission slots before body reading, per-app locks,
  streamed temporary files/capture blobs, blocking validation outside the gameplay
  lock, and short serialized budget/activation commits. An incomplete-upload test
  proves other reads continue, excess mutations receive `503 Retry-After`, and
  disconnects remove staging files. Structured errors and bounded busy retries
  are handled by the publisher; Infuser Play also reads structured errors.
- **Lifecycle:** independent bounded jobs, stale-result checks, verified upstream
  caching, separate maintenance, and catalog-authoritative registration recovery.
  Tests cover slow stops, late starts, interrupted publication, retirement restart,
  retained worlds and port reuse. Atomic firewall renewal passed an isolated
  nftables 1.1.6 UDP test: 50 renewals, no observed packet loss, expiry preserved.
- **Native:** coordinated verification/install/pruning, current/previous cache
  retention, process-identity-based run cleanup and copy-on-write with copy fallback.
  Tests cover a live orphaned game and owner/guest write isolation. Unknown legacy
  runs and uncertain spawn failures remain conservatively retained. Actual Windows
  installation/play acceptance still needs a person with that platform.
- **Routing:** both legacy and production-template Caddy journeys passed locally
  with Caddy 2.10.2, including authorization/header boundaries and WebSocket frames.
  Existing manual CI additionally exercises real Docker and managed packaging.

### Matched load result

Synthetic fixture on the same x86_64 Bazzite laptop, debug Rust builds, two Tokio
workers: 25 visible apps × 5 builds × 500 file entries, two WebSocket clients,
Library/chat polling and a concurrent 64 MiB upload/publish, capture enabled
with a 128 MiB quota. These are one-run observations, not latency guarantees.

| Measurement | Before (`b5154b9`) | After |
| --- | ---: | ---: |
| Frame round-trip p50 | 1.70 ms | 0.25 ms |
| p95 | 13.04 ms | 0.51 ms |
| p99 | 928.78 ms | 2.49 ms |
| Longest observed gap | 1,425.95 ms | 194.67 ms |
| Completed frame samples | 114 | 596 |
| Completed polling requests | 4 | 68 |
| Process high-water RSS | 320.2 MiB | 144.7 MiB |
| Process threads | 5 | 5 |
| Catalog DB / WAL bytes at sample | 4,141,056 / 6,060,552 | 4,161,536 / 6,085,272 |

The process includes the HTTP clients and their 64 MiB input buffer; RSS is not
daemon-only memory. A repeat in a temporary user service with `MemoryMax=512M`
and `TasksMax=128` passed, with 267.5 MiB cgroup peak including file cache and no
swap. Capture-disabled runs also passed. Installed production units have not
been changed or measured here; a binary release does not install service units.
This is not a claim of zero gameplay pauses: cold catalog commits, directory
fsync and first cache reconstruction still have cost. Lock-time histograms and
repeat native-launch IO measurements remain uncollected.

### Deliberate follow-ups

No new database service, full relational rewrite, lossy save durability, auth TTL,
Docker client crate, SSE, crypto consolidation or feedback-store merge was added.
Fork-specific feedback/documentation configuration remains a separate product
change; executable update trust must stay independent. These conditional items
do not block the measured improvements. Use the existing journeys before expanding
normalization or changing the remaining shared commit boundaries.

## Decisions with the largest impact

1. **Publishing must not interrupt gameplay.** Isolate upload, validation and
   packaging work from connection forwarding and session renewal. Bound background
   work rather than just increasing RAM and thread limits.
2. **Routine activity must not rewrite the catalog.** Separate presence and launch
   renewal from durable game declarations. Keep SQLite and the Rust daemon; a new
   database service or full schema rewrite is unnecessary to obtain this benefit.
3. **Lifecycle operations must recover independently.** A slow stop must not delay
   another game's start. Publishing, stopping and unregistering must converge
   correctly after a crash, including when database and controller writes disagree.
4. **Keep compatibility while changing internals.** Preserve game IDs, saves,
   guests, private groups, session admission and matched client/server updates.
   Storage migrations must account for automatic binary rollback and CLI writers.
   SDK 1.0 does not depend on finishing internal cleanup.
5. **Spend test effort on interactions.** A live game while another publishes,
   crashes between publication steps, and the real Caddy routing path are more
   valuable than a large new collection of isolated implementation tests.

These changes should fit the current single-host design. They do not introduce
per-group resource budgets, additional services, or release approval gates.

## What the review got right—and where to adjust it

| Review | Assessment from current code | Decision |
| --- | --- | --- |
| 2.1: whole-catalog reads and writes | Confirmed in `Database::read`, `edit` and `cleanup`. Library requests run cleanup and then another catalog edit, even during operator inspection. Sidecar upserts often avoid changing identical rows, but still perform scans and serialization. | Highest-value storage fix: remove activity writes from the catalog first; normalize more only with evidence. |
| 2.2: publishing stalls connections | Confirmed. Upload holds the global publication mutex during disk writes/fsync. WebSocket forwarding takes the same mutex during its two-second checks. Promotion validates files inside a database transaction. | Move expensive work off executor threads and out of shared critical sections; preserve publication/launch ordering. |
| 2.2: serial reconciliation | Confirmed. A slow Docker removal delays other games and the background loop's pending promotions/cleanup. | Bounded independent work per game, with attempt checks and retained reservations. |
| 2.3: Docker lookup per proxy request | Confirmed for internal-network HTTP backends. `web_address` invokes container listing and inspection. | Cache verified routing by runtime attempt; invalidate on lifecycle changes. |
| 2.3: OAuth and image caching | Repeated OAuth checks and revalidated art responses are real. A local OAuth check is not necessarily a Google request. The claimed cost needs measurement. Build-ID art URLs are not as strong as content hashes: deleted build IDs can be reused. | Revalidate art with ETags first. Defer auth caching and long-lived immutable caching until their benefit and access semantics are clear. |
| 2.4: unreclaimed registrations | Confirmed. Unpublish stops the server but leaves controller registration and port allocation; deleting builds does not release them. Sleeping published games retaining ports is intentional. | Add recoverable retirement of unused publisher registrations, preserving world data and operator-managed games. |
| 2.4: fragmented persistence | Multiple stores are real, but separation alone is not a defect. Captures, diagnostics and credentials have different retention and access needs. `promote` writing controller state inside a SQLite transaction creates a genuine crash boundary. | Define authoritative state and recovery first; do not merge every file into SQLite. |
| 2.4: Caddy test mismatch | Confirmed. `web_journey` exercises legacy generated routing. Runtime tests cover proxy-auth responses, but do not exercise the production Caddy composition. | Add a journey derived from the actual deployment template. Keep legacy operator behavior until it has an explicit replacement. |
| 3.1: service limits | The shipped 256 MiB limit contradicts the operator guide's 512 MiB. Upload bodies are collected before the handler lock, so serialized file writes do not bound total buffered memory. OOM/thread exhaustion is plausible, not demonstrated here. | Align limits and bound ingress/work concurrency; measure before choosing final limits. Do not adopt 1,024 tasks by default. |
| 3.2: packaging subprocess overhead | Polling spawns Docker and Node every 500 ms. However, `install.mjs` intentionally stays alive to preserve bounded tmpfs output; simply waiting for exit will not work. | Improve completion signaling later, preserving storage bounds and offline packaging. Defer replacing all Docker CLI calls. |
| 3.3: upstream URLs | Confirmed. A canonical upstream URL is not a private deployment detail, but fork support and feedback destination deserve explicit configuration. | Separate feedback destination, documentation source and trusted update source. Do not let a website silently redirect executable updates. |
| 3.4: native launch storage | Duplicate verification, full per-launch copies and missing game-cache pruning are confirmed. The UI already launches work on a background thread; “single-thread blocking” is not evidence of a frozen UI. | Prune safely and avoid duplicate verification. Preserve isolation; do not blindly hardlink writable game files. |
| 3.5: firewall renewal gap | Confirmed separate delete/add operations and possible duplicate concurrent grants. The suggested literal `nft update element` command is not in the current management-command grammar. | Test a supported atomic renewal transaction on supported host versions; coalesce requests without recording failed grants as successful. |
| 3.6: polling | Confirmed; most loops already pause when hidden, but admin network diagnostics does not. | Fix the hidden-tab omission. Prefer revisions/conditional responses before introducing SSE. |
| 3.7: organization/errors/crypto | Large modules and clients that only read string errors are confirmed. Overlapping crypto crates alone do not establish a maintenance or performance problem. | Fix error readers early; extract modules while changing their behavior. Defer crypto consolidation. |

Two important corrections to proposed remedies:

- Keep `synchronous=FULL` for durable state. WAL with `NORMAL` avoids corruption
  but can lose committed transactions after power loss or an OS crash. Reducing
  needless writes is preferable to weakening acknowledged save durability.
  [SQLite documents this distinction](https://www.sqlite.org/pragma.html#pragma_synchronous).
- Verify firewall syntax rather than copying it from the review. The
  [nftables manual](https://netfilter.org/projects/nftables/manpage.html)
  distinguishes set-element management commands from packet-path updates.
  Validate the chosen transaction in an isolated network namespace, including
  first grant and renewal after expiry.

## Build order and acceptance

### 1. Establish one useful baseline and close inexpensive gaps

Use existing dedicated-server and publishing fixtures. Add a short opt-in scenario:
two clients exchange frames while a different app uploads a maximum-size file
and publishes; Library/chat polling continues. Run with capture disabled and enabled.
Use synthetic catalogs at small and growing sizes, including the review's
25-app / 5-build / 500-file example. Never replay against the production catalog.

Record frame round-trip p50/p95/p99 and longest gap, catalog read/edit time and
count, lock wait time, Docker command count, process/cgroup peak memory and task
count, and database/WAL growth. Record workload and hardware with results. Avoid
logging tokens, request bodies or chat text. Existing tracing plus one script is
enough; a metrics service or player-facing telemetry is not needed.

In the same first batch:

- Make publisher and Infuser Play error readers accept both existing string errors
  and structured `error.message`, retaining status/code where available. Do not
  change old endpoint response shapes underneath published clients.
- Pause admin diagnostics when the tab is hidden.
- Align the shipped daemon unit and operator instructions. Use 512 MiB as the
  initial documented baseline to test, not a promise that it accommodates unlimited
  parallel uploads. Confirm installed units separately; a binary update does not
  automatically update systemd configuration.
- Add the production-template Caddy journey before changing upstream lookup.
  Use temporary ports and fake login, retaining the actual route order, header
  stripping and forward-auth behavior. No Google account is needed.

**Done when:** a repeatable baseline exists; both error formats remain useful;
hidden diagnostics stop; the production route passes authorized/unauthorized,
forged-upstream-header, HTTP and WebSocket cases. Check the daemon under its actual
service limits rather than calling a configuration change an OOM fix.

### 2. Remove the catalog from routine activity updates

Primary code: `src/platform.rs`, `src/portal/library.rs`, launch/heartbeat handlers,
connection validation, chat member lookup and `src/main.rs`.

- Move online presence to disposable in-memory state. It may briefly rebuild
  after daemon restart; operator inspection must never create presence.
- Put durable launch identity, scope and expiry in a dedicated table with targeted
  queries/updates. Keep launch renewal durable initially rather than introducing
  lossy batching and uncertain restart semantics at the same time.
- Remove Library-triggered cleanup; run maintenance on its own bounded schedule,
  independent of slow Docker operations. Authorization still checks expiry at
  request time; delayed cleanup must not extend access.
- Introduce narrow member/launch/catalog accessors instead of decoding all builds
  to authorize an asset or read chat. Start with the measured hottest callers.
- Run synchronous SQLite work through a bounded blocking boundary. Do not just
  wrap an entire async request in a blocking task or create an unbounded work queue.
- Preserve conditional save transactions and existing guest cleanup rules. Avoid
  reconstructing legacy `Data` on every call behind a supposedly optimized API.

**Done when:** Library polling, game heartbeats and connection renewals no longer
serialize or rewrite build inventories. A tenfold increase in retained build files
does not increase the number of rows/bytes touched by a presence or renewal write.
Existing save, guest, group, membership-disable and restart journeys pass.

**Migration requirement:** ship a compatibility release that understands the new
storage before activating the move. Both candidate and rollback binary must agree
on the authoritative tables. Test upgrade → new writes → rollback → upgrade using
both binaries, including the CLI member command. That command currently writes
SQLite outside the daemon, so an in-process cache cannot be authoritative without
an invalidation or writer-coordination design. If an older rollback cannot be
supported, use the documented maintenance path; do not silently dual-write forever.

### 3. Isolate publishing work from live sessions

Primary code: upload/promotion handlers, `src/portal/server_builds.rs`,
`src/portal/capture.rs`, `src/publish_capture.rs`, connection and presence handlers.

- Admit a bounded number of uploads **before** collecting their bodies. Stream
  accepted uploads into private staging files, enforcing file/content/disk limits
  while reading. Cancellation and failures remove staging files and reservations.
- Preserve exact request capture without buffering the same large payload through
  middleware and SQLite. Spool authenticated input once, hash incrementally and
  give capture bounded access to it. Keep capture gaps/unknown outcomes explicit;
  capture failure must not become a publishing failure.
- Use per-app publication coordination and short metadata commits. Keep a separate
  atomic host-wide content reservation: per-app locks alone permit simultaneous
  apps to exceed the global disk budget.
- Seal inputs before validating outside the metadata lock. Recheck build/app
  generation before committing so concurrent upload, deletion or promotion cannot
  invalidate a successful validation. Move hashing, decoding and fsync off Tokio
  executor workers with bounded concurrency.
- Keep launch/promotion atomicity for matched web/server builds. Connection checks
  must not wait behind artifact IO, including an update to their own game. A new
  async mutex alone only avoids blocking a worker; it does not remove this wait.

**Done when:** the two-player fixture remains connected while another app uploads,
validates and publishes, and a same-app update queues without changing the active
client/server pair. Deterministic slow-storage tests prove forwarding and renewals
make progress before the write completes. Load runs report before/after latency
and bounded memory with capture both on and off. No fixed millisecond promise is
made until the baseline exists.

### 4. Make server lifecycle independent and recoverable

Primary code: `src/controller.rs`, `src/docker.rs`, `src/store.rs`, promotion and
unpublish handlers, runtime/connection routing, firewall helper.

- Dispatch bounded per-app lifecycle operations, initially a small concurrency
  such as two. Preserve global RAM/CPU reservations and one operation per attempt.
  A slow task must not impose a barrier before the next round of ready work.
  Apply completions only if app, attempt and expected phase still match.
- Cache a verified backend address for the current runtime attempt. Clear it
  before stopping/replacing that attempt; rebuild it after restart/adoption.
  Never trust a persisted address without validating the owned running container.
- Persist desired publication/retirement, then reconcile Docker/controller state
  idempotently. Document recovery from each write boundary. Avoid pretending a
  SQLite transaction makes `state.json`, filesystem and Docker changes atomic.
- Retire unused publisher registrations after no active/queued server build or
  live reservation needs them and cleanup is confirmed. Reclaim obsolete web-only
  registrations too. Retained artifacts can re-register when published again.
  Do not remove configured operator games, persistent world volumes or saves.
- Fix firewall renewal with an atomic, version-tested transaction. Coalesce
  in-flight grants per address; all waiters see failure and can retry. Retain the
  kernel expiry and shared-IP behavior.

**Done when:** a blocked stop for A does not prevent B becoming ready; stale task
completion cannot resurrect A; requests to a ready backend require zero Docker
CLI calls in steady state. With a tiny port pool, repeated publish → unpublish →
remove cycles do not exhaust ports, including restart mid-retirement. Crash tests
around promotion recover a matching web/server build. Firewall renewal does not
introduce packet loss in a continuous UDP test, and inactive grants still expire.

### 5. Bound Infuser Play storage before broader native adoption

Primary code: `desktop/src/client.rs`, `desktop/src/install.rs`.

- Establish one verification/install/run-preparation operation under coordinated
  cache locking. Remove duplicate verification without opening a race with cache
  repair or another owner/guest launch.
- Retain current and previous useful versions plus anything needed by an active
  run. Sweep abandoned runs conservatively. Parent-process death alone does not
  prove the game has exited; uncertain liveness must not delete a running game.
- Prefer copy-on-write cloning where supported, with ordinary copies as the
  portable fallback. Hardlinks are not copy-on-write: a game's write can alter the
  cache and other runs. Keep per-run files, owner saves and guest saves separate.

**Done when:** repeated updates have bounded cache growth, concurrent owner/guest
launches and cache repair remain isolated, interrupted installs recover, and
cleanup never deletes saves or a live run. Measure repeat-launch IO. Exercise
Bazzite and Windows; CI compilation alone is not Windows user acceptance.

### 6. Reassess the remaining optimizations using the measurements

| Candidate | Trigger and preferred approach |
| --- | --- |
| Remaining catalog normalization | If cold catalog reads/writes still dominate, migrate apps/builds/file inventories behind narrow accessors, one boundary at a time. Introduce a schema version, backup/restore rehearsal and compatible rollback plan. Do not require a full relational conversion first. |
| Art and chat traffic | Add authorized ETag revalidation. Chat revisions must reflect deletions, retention expiry and member/profile/group changes, not just the newest message ID. Library presence changes with time, so catalog revision alone is insufficient. |
| OAuth cache | Only if measured local auth cost matters. Bound the cache, preserve refreshed cookies, and keep membership/group authorization current. Define logout/expiry behavior before adding a TTL. |
| Packaging completion | Replace repeated Node probes with a completion signal/read from the existing bounded container. Do not replace bounded tmpfs with unrestricted writable host storage. |
| Docker Engine client crate | Consider behind `Engine` after request-path caching. Adopt only if remaining CLI overhead or maintenance justifies a migration; preserve timeouts, ownership checks and uncertain-outcome recovery. |
| Feedback/capture storage | Separate storage is acceptable. Consolidate feedback only if it simplifies demonstrated consistency/maintenance problems. Optimize capture connection lifecycle and account for WAL growth if WAL is enabled; retain its independent quota and failure policy. |
| Fork configuration | Separate operator-controlled feedback/docs settings from desktop update trust. Keep current upstream defaults and backward compatibility. |
| Module extraction | Extract publishing/storage/lifecycle boundaries as they change. Avoid an unrelated large move/format commit or a target line count. |
| SSE and crypto consolidation | Defer unless polling or dependency measurements show a real problem. They do not address the strongest findings. |

## Extract lasting value from the review

Use the numbered batches above as the backlog, with links to the originating
review sections. For each completed batch, record the implementation commit,
one before/after result, the regression journey and any remaining limitation here.
“Deferred” remains a deliberate decision with a trigger, not a forgotten finding.

Reuse the existing publication capture/replay journey and frozen legacy SDK
fixture for importer and compatibility work. Selected real publication captures
can become private regression inputs after inspection; raw artifacts and private
metadata do not belong in public test fixtures. Do not build a general replay
product merely to carry out this review.

Run fast deterministic checks with the affected work. Keep Docker/Caddy, network
namespace and load runs opt-in and use them at the relevant milestones. No new
approval requirement or mandatory slow PR lane is needed. Keep reports for the
operator/developers, not in the player UI.

Also correct the developer architecture summary alongside the relevant work: it
still says one gameplay session per app and no dedicated joinability reporting,
while the accepted contract supports multiple sessions inside one backend and
implemented reporting. This is a documentation defect, not a reason to change
the working SDK.

Stop expanding this program once the important properties are demonstrated:
publishing does not stall play, routine activity avoids catalog rewrites, lifecycle
failures stay local, storage remains bounded, and upgrade/rollback preserves games
and saves. Continue real-game acceptance and SDK stabilization from the
[roadmap](../roadmap.md) throughout; internal polish must not displace that work.
