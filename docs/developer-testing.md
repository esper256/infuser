# Feature tests and manual checks

Audience: Infuser developers.

Start with [development setup](development.md). Run commands from the repository
root. These harnesses test the platform. Historical results below record what was
exercised, not certification of every game or device.

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
The harness binds only loopback, uses synthetic members and deletes temporary data
on exit. Its ready file contains a test publishing credential: keep it private.
Production tests still use the real Docker/Caddy deployment.

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
