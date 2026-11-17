# Feature tests and manual checks

Audience: Infuser developers.

Start with [development setup](development.md). Run commands from the repository
root. These harnesses test the platform. Historical results below record what was
exercised, not certification of every game or device.

## Unity Web foundation

For C# compilation, installed-package Editor tests, and the acceptance boundary,
see [Unity adapter checks](development.md#unity-adapter-checks).

`node --test tests/unity_sdk.mjs` tests the JavaScript bridge and ES5 interop
boundary: hosted/standalone selection, cloud retries/conflicts, exact Join Friend
targets, late admission, disconnects, binary/text WebSockets and congestion.
These run in the existing fast SDK lane without a Unity installation.

`cargo test --test publisher_journey --test compatibility_journey` checks explicit
Unity exports, missing bridge diagnostics, authenticated gzip/Brotli asset headers,
`.unityweb` fallback handling, byte ranges, HEAD, and legacy precompression.
With existing Playwright and Chromium installations, add:

```sh
INFUSER_UNITY_BROWSER_NODE=/path/to/node \
INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs \
INFUSER_CHROMIUM=/path/to/chromium \
cargo test --test compatibility_journey
```

That optional journey uses a disposable controller and real local HTTPS proxy.
Chromium loads the actual bridge/SDK, streams gzip and Brotli WASM, runs cloud
save/conflict/resolution/reload flows, isolates guest saves, verifies cross-origin
isolation, and rejects expired authentication. The WASM fixture is a minimal module,
**not Unity**. No production account, Editor, network relay or game is used.

Before advertising a validated Unity integration, build Beacon Garden with the
target Editor and Web module. Verify IL2CPP compilation, stripping and callbacks;
publish it unlisted; run the save and guest journeys in the actual player. Then
test two clients (including forced TURN and actual traffic in both directions),
Join Friend, host departure, background/resume and the dedicated socket bridge
against a real server. Repeat gzip/Brotli/fallback exports and any threaded export
the supported Unity version can produce. Test Bazzite/Windows browsers and a
real existing game; record exact Editor/package versions. Do not infer Unity
package compatibility from the bridge's mock transport tests.

## Unattended operation

`node tests/server_updates_journey.mjs` uses the existing `INFUSER_PLAYWRIGHT` and
`INFUSER_CHROMIUM` paths to check the operator's Admin tools update controls,
confirmation, single submission, elapsed wait and stale/offline/failure states.
The HTTP authorization and replay checks are in `groups_journey`; `update_journey`
checks staging during maintenance and refusal to activate a superseded candidate.
These tests use disposable state and do not authorize a production restart.

`node tests/reboot_journey.mjs` runs the actual reboot scripts against a local
HTTP admission fixture with fake time and systemd. It checks absent reboot flags,
window boundaries, package activity, busy admission, shared-lock exclusion,
inhibitors, failure cleanup and successful shutdown requests. Requires Bash,
curl, jq and flock; it never executes the host's reboot command.

Build `docker build -t infuser-caddy:acceptance deploy/services/caddy`, then run
`bash tests/service_images_journey.sh` for the DNS module/HTTP and service binary
smoke checks, including two domains and adding/removing one site's configuration
without changing the other. The manual Docker/Caddy workflow uses this same build for its full
authentication and forwarding journey. The scoped host-maintenance workflow
gives early feedback on image and reboot-script PRs.

Fast checks: `cargo test --locked --test operations_journey --test host_health_journey --test update_journey`.
These cover maintenance admission, active-player deferral, bounded recovery,
planned backup downtime, the recovery hook, staged-release cleanup and rollback.
`groups_journey` verifies operator-only health access; set
`INFUSER_GROUP_BROWSER_NODE=/path/to/node` to include its browser flow.

The encrypted restore journey is opt-in:
`INFUSER_TEST_RESTIC=/path/to/restic cargo test --locked --test backup_journey -- --ignored`.
It verifies replacement retention, restored files/worlds and a restored game and
save served through HTTP. Use restic 0.19.1 or newer for the tested command set.

The manual Docker/Caddy integration workflow also runs a root-only backup fixture
against uniquely owned volumes and the demo image. It checks surviving-container
rejection, required images, world ownership, permissions, links and directories.
Its `lifecycle_rounds` input enables 2–100 real wake/serve/sleep cycles without
slowing PR CI. Twenty cycles are a useful first endurance check; they are not a
multi-day soak or proof that every game's save format tolerates abrupt shutdown.

Run `bash tests/monitoring_journey.sh` on Linux with Docker, curl and jq. It starts
resource-limited, disposable Grafana/Prometheus containers and a local webhook
receiver using the images selected by the monitoring Compose file. It checks
actual firing/recovery delivery, bearer-token handling, separate heartbeat
routing, and heartbeat cutoff/recovery when collector data goes stale. The fixture
includes an unrelated failed project and a separate policy tree:
Infuser alerts stay healthy, other-project notifications reach their own receiver,
and provisioning reload preserves that tree. Shortened
evaluation intervals keep it to a few minutes; it is opt-in, outside normal CI.
No real notification destination or production state is used. Ports 13000, 19090
and 19095 must be free; `INFUSER_TEST_GRAFANA_PORT`, `INFUSER_TEST_PROMETHEUS_PORT`
and `INFUSER_TEST_WEBHOOK_PORT` override them.

Production acceptance additionally needs an ARM64 memory measurement, a real
destination test and successful scheduled heartbeat delivery. Confirm phone
receipt and independent outage notifications with the operator. Heartbeats do
not test inbound HTTPS from another network. Never restart the production Docker
engine to exercise a test scenario.

## Visits

`cargo test --locked --test visits_journey --test connections_journey --test peer_journey --test native_journey`
checks visit grants, isolated deployments, chat windows, queued notifications,
live dedicated cutoff, peer rooms and native client capability admission.
The companion suite additionally starts a real process and stops it after an
authorization denial. `node --test tests/peer_sdk.mjs tests/peerjs_sdk.mjs` covers
lost signaling, late responses and sleep where only the wall clock advances.

With Playwright installed, set `INFUSER_VISITS_BROWSER_NODE=/path/to/node` when
running `visits_journey` to include the real operator/visitor website journey.
`INFUSER_PLAYWRIGHT` and `INFUSER_CHROMIUM` select existing browser installations.
All accounts, visits and games in these fixtures are disposable; no Google login
or production membership is used.

## Library refresh performance

Run the mocked browser journey with Playwright and Chromium:

```sh
INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs \
INFUSER_CHROMIUM=/path/to/chromium node tests/library_refresh_journey.mjs
```

It loads 40 games and 40 friends in both themes at 320–1200px, checks that resize
observers settle at idle, and requires unchanged library/chat responses to cause
zero DOM mutations or layouts. It also checks live updates, retained focus and
chat drafts, menu stability, a real 12-second library timeout and recovery, and
single-request/background limits for admin diagnostics. Browser task time is
reported rather than asserted because it depends on the machine. This does not
profile game tabs, extensions, GPU load or the user's whole computer.

## Docker and Caddy

`cargo test --locked` runs the fast API lifecycle journeys. To exercise Docker too:

```sh
docker build -f examples/demo/Dockerfile -t infuser-demo .
INFUSER_TEST_IMAGE=$(docker image inspect infuser-demo --format '{{.Id}}') \
  cargo test --locked --test docker_journey -- --ignored --nocapture
```

The Docker journey uses temporary state and loopback-only game ports. Run it as a
user with access to the Docker socket. It does not use a production catalog.

The optional Caddy journey runs the generated routing on temporary local HTTP
ports with a test authentication service and backend:

```sh
CADDY_BIN=/path/to/caddy cargo test --locked --test web_journey -- --ignored
```

It verifies the auth gate, SPA navigation, missing assets, WASM MIME type,
cross-origin headers and backend routing. It does not contact Google or issue
real TLS certificates.

A second journey derives routing from `deploy/Caddyfile.example`, substituting
only local listeners, TLS setup and test upstreams. It checks authentication,
forged headers, credential/cookie stripping, path/query forwarding and text/binary
WebSocket frames through the production route composition.

## Publishing and recovery under load

The normal suite includes `activity_storage`, `lifecycle_independence` and
`server_packaging_journey`: activity writes leave the cold catalog unchanged,
external writers invalidate cached membership, slow stops do not block other
starts, late starts cannot undo stops, retirement reclaims ports, and incomplete
uploads neither fill request memory nor block status reads. A restart after
durable unpublish must retire registrations while preserving worlds and builds.

To test the migration with an actual previous executable:

```sh
INFUSER_PRE_GENESIS_BINARY=/path/to/pre-genesis/infuser \
  cargo test --locked --test activity_storage
```

This creates disposable state using the old CLI, migrates to identity genesis,
renews a guest launch, proves the old CLI can no longer write, and reopens with
the current code. `identity_journey` covers stable Google subjects, email changes,
preserved saves, profile isolation and version rejection. `update_journey` covers
the rollback floor during failed activation and interrupted recovery.

The identity journey also tests cross-group invitations, explicit profile
selection, stale/foreign selectors, artwork URLs, chat isolation and unchanged
game tickets/saves after switching. Set `INFUSER_PROFILE_BROWSER_NODE` to a Node
executable to include `tests/profile_chooser_journey.mjs`. It exercises the
single-profile bypass, desktop/mobile chooser, independent tabs, Switch Profile
and the Infuser Play pairing handoff. `INFUSER_PLAYWRIGHT` and `INFUSER_CHROMIUM`
can select locally installed browser tooling.

For login lifetime regressions, use the deployed OAuth2 Proxy version's binary
(verify its release checksum) and existing Playwright/Chromium installations:

```sh
INFUSER_OAUTH2_PROXY=/path/to/oauth2-proxy \
INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs \
INFUSER_CHROMIUM=/path/to/chromium \
  node tests/login_session_journey.mjs
```

This starts an isolated OIDC provider and proxy on loopback. It reproduces expiry
without a refresh token, then exercises the shipped minimal-cookie settings:
existing cookies survive adoption, reloads/new tabs/concurrent requests retain
the login, identity headers remain available, and forged/expired cookies fail.
Token and cookie lifetimes are shortened to keep the test under a minute. It
uses no Google credentials or production state. The provider is a fixture; this
does not substitute for checking Google sign-in on an actual Apple device.

The optional local load case exchanges frames between two clients while uploading
and publishing a 64 MiB file, with Library/chat polling. The larger synthetic
catalog has 25 apps, five builds each and 500 files per build:

```sh
INFUSER_REVIEW_LOAD=1 INFUSER_REVIEW_LARGE_CATALOG=1 \
INFUSER_REVIEW_CAPTURE_MB=128 \
  cargo test --locked --test connections_journey -- --nocapture
```

Omit `INFUSER_REVIEW_CAPTURE_MB` to disable capture; omit the large-catalog flag
for the small fixture. Results include round-trip percentiles, longest gap, polling
progress, process high-water RSS, thread count and database/WAL sizes. The process
includes clients and their upload buffer; these are not production daemon or WAN
measurements. Run the compiled test in a transient user service with
`MemoryMax=512M` and `TasksMax=128` to exercise the shipped limits without changing
installed services. See the [review results](proposals/architecture-improvement-plan.md).

## Firewall renewal

Compile `cargo test --locked --test firewall_journey --no-run`, then run its
reported test executable in a new user/network namespace:

```sh
unshare --user --map-root-user --net env INFUSER_FIREWALL_NAMESPACE=1 \
  /absolute/path/to/firewall_journey-TEST_HASH --ignored --nocapture
```

The namespace needs `nft` and `ip`. From a distrobox, prefix that command with
`distrobox-host-exec` if those tools are on the host. Never run it in the host's
network namespace. It tests first grant, expiry, repeated atomic renewal during
UDP traffic and IPv6 grants. Tested locally with nftables 1.1.6; repeat on the
deployment's supported version when changing the rule generator.

## Native cache and run cleanup

`cargo test --manifest-path desktop/Cargo.toml --locked` checks update integrity,
owner/guest copy isolation and pruning while an orphaned game process remains
alive. It uses a sleeping subprocess fixture without running a real game.
Current/previous versions and uncertain live runs survive; known-dead runs are
removed. This supplements, rather than replaces, Windows/Bazzite installation,
browser handoff and real-game acceptance.

## Dedicated Node example

Stage the [Node example](../examples/dedicated-node/README.md) with
`node examples/dedicated-node/prepare.mjs`. Run `npm ci --ignore-scripts` in
`.local/dedicated-sdk-project/server`, then start the fixture and browser test
in separate terminals:

```sh
INFUSER_TEST_NODE=node cargo run --example dedicated_fixture -- \
  .local/dedicated-sdk-project .local/dedicated-ready.json
node tests/dedicated_sessions_journey.mjs .local/dedicated-ready.json
```

The browser test requires Playwright from `services/peerjs` and an installed Chromium.
Set `INFUSER_PLAYWRIGHT` and `INFUSER_CHROMIUM` to use other local installations.
The controller API binds to loopback, uses synthetic members and deletes temporary data
on exit. Its ready file contains a test publishing credential: keep it private.
Production tests still use the real Docker/Caddy deployment.

### Disruption soak (developer machines, Linux only)

Use Node 24. After staging the example and installing its dependencies above:

```sh
cargo build --locked --example dedicated_fixture
node tests/reliability_soak.mjs > reliability-soak.jsonl
```

This opt-in test runs for two hours; it is not part of PR or release CI. It owns
two browser players and an isolated guest, a real controller/router process,
and a Node game server. It uses only synthetic accounts and a fresh temporary
state directory. No production URL, Google login, Docker daemon, or game is used.

The test drops a save response, interrupts file uploads, disconnects transports,
freezes a renderer for 35 seconds, kills the game server, and kills the controller
and its backend together. It checks continued gameplay, exact-room reconnects,
honest rejection of lost rooms, checkpoint retry receipts, save isolation, and
cleanup of incomplete uploads. Failed servers retain the real 30-second backoff.
Gameplay is checked every 15 seconds between disruptions, including across server
credential renewal. Each cycle records controller memory, file descriptors and
database size as JSON lines.
The final check verifies SQLite integrity before marking the run passed.

For a short development check, use
`INFUSER_SOAK_SECONDS=60 INFUSER_SOAK_INTERVAL_SECONDS=0 node tests/reliability_soak.mjs`.
It always completes at least one four-disruption cycle, so the short run takes
longer than a minute. The default interval between disruptions is 120 seconds.
Use `INFUSER_SOAK_FAULTS=transport,background` to keep the same controller and game
process alive throughout a run, checking credential renewal and resource growth
without resetting them during crash tests.
Successful runs remove their temporary state; failures print its location and
retain the fixture log. `INFUSER_KEEP_SOAK_STATE=1` also retains successful runs.
Those files contain disposable credentials; keep them private. Ctrl-C stops the
owned process group and browser. Do not point the fixture at deployment data.

This test uses a process-based engine adapter, not Docker. Run the manual
`integration.yml` workflow separately for real container lifecycle, persistent
volumes, encrypted restore, packaging and Caddy. Renderer freezing is not physical
laptop sleep, Chromium is not Safari, and these dedicated WebSocket tests do not
validate TURN renewal or peer-to-peer recovery.

### Recorded acceptance (2026-11-17 UTC)

- Two hours passed 50 disruptions: 13 transport disconnects, 13 renderer freezes,
  12 game-server crashes and 12 controller/process-group crashes. Checkpoint
  retries, guest isolation, interrupted-upload cleanup and continued gameplay
  passed. Controller RSS peaked at 43.03 MiB; sampled file-descriptor counts were
  20–34. The retained database passed SQLite integrity checks, and recorded cloud
  save usage matched stored bytes.
- An hour with the same controller and game process passed 27 transport/background
  disruptions in the same multiplayer room. Controller RSS settled near 44 MiB (44.34 MiB
  peak); sampled file-descriptor counts were 23–32. The retained database passed
  SQLite integrity checks, and recorded cloud save usage matched stored bytes.
- [100 real Docker lifecycle cycles](https://github.com/esper256/infuser/actions/runs/44011607500)
  passed alongside world/image snapshots, encrypted restore, managed Node hosting
  and Caddy. A [second run](https://github.com/esper256/infuser/actions/runs/44014205151)
  also passed deliberate container death and controller reopen while preserving
  a surviving container's memory and lease.
- Browser journeys passed offline recovery, exact-room reconnect, concurrent
  last-seat admission, queued publishing, credential renewal, profile switching,
  visits and OAuth-proxy login continuity. Save conflicts, access isolation,
  publisher retries, update rollback and large-upload responsiveness checks passed.

The long runs used runtime baseline `0a01f0f`. The combined UI through `184ac9e`
also passed the short crash cycle and extended browser journey, including its
prompt to choose a profile picture. These are disposable Linux/Chromium fixtures;
the memory figures are local measurements, not production capacity guarantees.

## Godot web example

Export [Lantern Lake](../examples/godot-web/README.md) before running these checks.
The ordinary Rust/JS suite stays fast; browser journeys are explicit. Install
Playwright in a private development directory and install its Chromium browser.
Set `INFUSER_PLAYWRIGHT` to the absolute `playwright/index.mjs` path if needed,
and `PLAYWRIGHT_BROWSERS_PATH` for a non-default browser installation.

For local save tests, create a disposable TLS certificate, for example:

```sh
mkdir -p .local/godot
openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj /CN=games.example.com \
  -keyout .local/godot/key.pem -out .local/godot/cert.pem
```

Then run in separate terminals, starting a fresh fixture for each run:

```sh
cargo run --locked --example godot_fixture -- examples/godot-web/export
node tests/godot_web_journey.mjs
```

The fixture binds loopback, uses a temporary database and local mock login, and
is discarded when stopped. A real TLS proxy covers every asset request, including
Godot AudioWorklets. The journey checks:

- Saved progress survives reload and a new non-guest launch.
- Guest progress survives refresh, a new guest starts empty, and owner saves
  remain unchanged.
- Two stale writers preserve both conflicting checkpoints.
- A rejected save stays visibly unsaved; retry reuses the same commit ID.

For live TURN, use the operator's `preview-game` command to mint two fresh guest
launches for this hidden app. Store their URLs in a private JSON file:
`{"urls":["FIRST_LAUNCH_URL","SECOND_LAUNCH_URL"]}`. Never commit that file.

```sh
node tests/godot_relay_journey.mjs /private/previews.credentials.json
# Mint two fresh grants before running again:
node tests/godot_relay_journey.mjs /private/previews.credentials.json direct
```

Both modes check actual RPC reception, selected candidate pairs and byte counters,
one shared epoch and host departure ending the session. The forced run requires
relay/relay; automatic mode reports its selected working path. These tests execute
real WASM. Browser instrumentation is not exported with the game. Screenshots go
under ignored `.local/artifacts/`.

Bazzite's installed Flatpak Chrome passed both export variants, recovery and
targeted Join Friend. `recovery` mode accelerates TURN renewal, injects one failed
refresh, closes a transport, backgrounds the host and suspends its renderer past
the lease. This is not physical laptop sleep or a full-hour soak. Windows browser
acceptance remains pending. See the [engine study](engines/godot-web.md)
and [native preview](engines/godot-native.md).

## Native Godot

### Windows handoff checklist

1. Open the normal installer under a non-admin Windows account. No unzip step.
2. Press Play on a native preview in the Library; allow the Infuser Play link handler.
3. Approve the browser login handoff; confirm the game opens under the right player.
4. Save, close and reopen. Then use guest mode: blank progress, owner preserved.
5. Publish an update. Next Play downloads it; saves remain. A running older game
   stays intact. Try a disconnected launch and confirm the error is understandable.
6. Close the game and confirm Infuser Play exits. Uninstall/reinstall the companion and
   confirm personal saves remain. Check a laptop with scaling enabled too.

Actual Windows testing is pending; Linux results do not substitute for it.

The opt-in `tests/godot_native_journey.mjs` uses a loopback TLS fixture and an
export of the adapted Dodge project. On the Bazzite laptop it passed verified
download, real native Godot startup, cloud checkpoint/reload, persistent owner
user://, blank guest user://, guest cleanup and unchanged owner progress. Its test
certificate and acceptance autoload belong only in that disposable project.
`desktop/examples/acceptance.rs` supplies that CA to a test client; the installed
companion has no test-certificate override. Archive corruption/update isolation
and device-pairing/claim/revocation are also covered by fast Rust journeys.
The CI-built Flatpak was installed and started on Bazzite with the standard
Freedesktop 25.08 runtime, and its `infuser://` handler was registered. The Windows
installer compiled successfully and desktop unit journeys passed on the Windows
CI runner. Google login plus a full installed-app play session still needs a
human account acceptance pass; mock login tests do not certify that interaction.

## Peer SDK recovery

`node --test tests/*_sdk.mjs` runs the shipped SDK against controllable signaling
and WebRTC boundaries, with a simulated clock. It covers transient signaling
failure, link replacement, tab sleep/lease expiry, leaving during host/member
negotiation, stale events, and send/close races without real-time sleeps. These
checks complement the HTTP peer journeys and live two-browser TURN dogfood;
they do not claim to simulate a browser's actual ICE implementation.

## PeerJS

The regression suite checks gateway authorization and fast adapter recovery.
For a full local browser run, `npm ci --prefix services/peerjs`, install its
Playwright Chromium if needed, build `cargo build --example peerjs_fixture`, then
run `node tests/peerjs_journey.mjs`. The fixture binds only loopback and uses fake
accounts; it is never compiled into the production daemon. A private file with
two preview launch URLs can instead drive `node tests/peerjs_journey.mjs FILE recovery`
against real TURN. Never commit or print those launch URLs.

## SDK compatibility

The frozen `tests/fixtures/legacy-sdk` snapshot is an old client, not another
implementation to keep editing. A real HTTP journey executes it unchanged
against the current daemon, covering launch, identity, save/reload, guest refresh,
guest isolation and renewal, then verifies shared save ownership with major 1.
Both served SDK generations also run the fast cloud/recovery checks.

Run `cargo test --locked` with Node 24 on PATH, or set `INFUSER_TEST_NODE` to its
absolute executable path. `node --test tests/*_sdk.mjs` runs the clock-controlled
SDK checks. Node is a development/test dependency, not a controller dependency.

## Managed Node packaging

The representative ws/Three.js fixture has passed:

- API/CLI journeys for sealed uploads, invalid dependency URLs, capacity queuing,
  interruption/retry, cleanup, and adding multiplayer to an existing app.
- Real Docker installation with lifecycle scripts disabled, readiness checks,
  two WebSocket players, departure, idle shutdown, and a fresh restart.
- Live ARM64 Node 22/24 packaging from a laptop without Docker. Two Chromium
  guest launches covered cold title screens, shared and independent rooms,
  reconnects, isolated saves, CPU/RAM limits and blocked Internet/metadata access.
- A broken server leaving the active pair intact, a valid update waiting for
  players to leave, and the updated pair waking and sleeping again.

This validates the hosting path, not an individual game's integration or maximum
player capacity. [Dedicated sessions](dedicated-sessions.md) adds verified identity,
session discovery and Join Friend as explicit capabilities on top of hosting.

## Chat and notifications

Checks: `cargo test --locked --test chat_journey` exercises real HTTP/SQLite,
permissions, retry/removal, bounded storage, profile changes, and game activity.
Browser review covers both themes, narrow phones, two member sessions, drafts,
unread state, scrolling, reconnects, and automatic refresh deferrals.
`node tests/chat_scroll_journey.mjs` is the focused browser regression for tiny
upward wheel gestures, keyboard scrolling and history anchoring. It uses Playwright
with a local mock API; set `INFUSER_PLAYWRIGHT` to its module path and optionally
`INFUSER_CHROMIUM` to a system Chromium executable when not using their defaults.

Checks: `cargo test --locked --test notifications_journey`. To include the actual
Chromium service worker, notification, fullscreen and sign-out journey, set
`INFUSER_PWA_BROWSER_NODE=/path/to/node` (use the Playwright setup in
[development](development.md)). The test substitutes browser-provider registration;
actual iPhone/Android background delivery must also be checked on a phone.

Channel storage and access: `cargo test --locked --test chat_channels_journey`.
This upgrades populated legacy stores, including active visits and queued push,
checks atomic migration failure/retry, and verifies explicit participant and
channel expiration through the same policy. Run `chat_journey`,
`notifications_journey`, and `visits_journey` alongside it for the unchanged
website contract. See [channel foundations](proposals/chat-channels.md) for the
storage compatibility and rollback boundary.

## Feedback

Tests: `cargo test --locked --lib portal::feedback`,
`cargo test --locked --test feedback_journey`, and
`node tests/feedback_journey.mjs` with Playwright installed (or
`INFUSER_PLAYWRIGHT` set to its module path; optional `INFUSER_CHROMIUM`).

## Godot web validation

This adapter is a preview. The real Godot/WASM
[Lantern Lake journey](../examples/godot-web/README.md) covers cloud saves,
guest refresh/isolation, conflicts, save retries, and forced-relay multiplayer.
Tested: Godot 4.7.2 standard/GDScript, Compatibility renderer, both single-threaded
and threaded web exports. Bazzite's installed Flatpak Chrome passed real Godot
RPCs through TURN, brief outages, connection replacement, background operation,
credential refresh with a transient failure, and renderer suspension beyond the
room lease. Tests used headless Chrome; physical laptop sleep and actual Windows
browsers remain acceptance items. This is not an SDK 1.0 declaration.

## Godot multiplayer validation

Lantern Lake uses real `WebRTCMultiplayerPeer` channels and GDScript RPCs.
The relay journey checks selected candidate pairs, bytes and RPC reception in
both directions, and host departure. An automatic-mode run also passed with a
direct connection. Normal gameplay has no numeric network overlay.

Recovery passed in Bazzite's Flatpak Chrome for threaded and single-threaded
exports: a short signaling failure, transport replacement, background operation,
TURN renewal with one failed refresh, and renderer suspension beyond the lease.
The test accelerates the renewal deadline using real credentials; it does not
replace a full-hour soak or physical laptop sleep. Windows browser checks remain
pending. See [Godot web example](#godot-web-example) for commands.

## Existing Godot project fixture

Run the [Godot web fixture](#godot-web-example) with
`--project PATH_TO_ADAPTED_PROJECT` after exporting the
[adapted project](../examples/godot-existing/README.md).
The fixture's app ID remains `lantern-lake`; its title and save declaration come
from the supplied existing project. No real account, public game listing, or
upstream repository change is required for this study.

## Ansible host maintenance

Install the pinned operator tooling in an isolated environment, then run the
opt-in journey (no root or Docker required):

```sh
python3 -m venv .local/ansible-venv
.local/ansible-venv/bin/pip install -r deploy/ansible/requirements.txt
ANSIBLE_COLLECTIONS_PATH="$PWD/.local/ansible-collections" \
  .local/ansible-venv/bin/ansible-galaxy collection install -r deploy/ansible/requirements.yml
ANSIBLE_COLLECTIONS_PATH="$PWD/.local/ansible-collections" \
INFUSER_TEST_ANSIBLE="$PWD/.local/ansible-venv/bin/ansible-playbook" \
INFUSER_TEST_PYTHON="$PWD/.local/ansible-venv/bin/python" \
  node tests/ansible_host_journey.mjs
```

It exercises real Ansible preview/apply/idempotence, private diff suppression,
per-file validation and retained backups in disposable directories. A local HTTP
fixture and fake systemd exercise busy admission, lock exclusion, failed apply
and independent recovery after a killed worker. It does not certify real systemd
or Docker Compose compatibility; verify those on the supported host separately.

With Docker available, run the separate disposable Compose compatibility check:

```sh
ANSIBLE_COLLECTIONS_PATH="$PWD/.local/ansible-collections" \
  .local/ansible-venv/bin/ansible-playbook -i localhost, tests/ansible_compose_journey.yml
```

It verifies selected-service creation, idempotence, check-mode non-mutation and
preservation of an unselected neighbor, then removes only its temporary project.

## Voice chat

`cargo test --locked --test voice_journey` exercises real Infuser login and
channel authorization with a disposable media-control fixture. It checks group
isolation, visit admission/revocation, read-only inspection, forged credentials,
reconnect denial, private control requests, one connection per profile, control
service failure and orphan removal after controller restart.

For actual media, obtain the official LiveKit 1.13.11 binary for your architecture
and verify its release checksum. With the existing Playwright/Chromium setup:

```sh
cargo build --locked --example voice_fixture
INFUSER_TEST_LIVEKIT=/absolute/path/to/livekit-server \
INFUSER_PLAYWRIGHT=/absolute/path/to/playwright/index.mjs \
INFUSER_CHROMIUM=/absolute/path/to/chromium \
  node tests/voice_journey.mjs
```

This short, opt-in journey launches a disposable LiveKit server and controller,
uses synthetic microphones, and asserts decoded audio energy in both directions.
It checks real UI badges, cross-tab off, group isolation, visit cutoff and mobile
control placement. The fixture login and mutation routes exist only in the example;
never expose them publicly. This does not replace actual Safari/iPhone microphone,
physical sleep or restrictive-network/TURN acceptance.
