# Development checks

Audience: Infuser developers.

This page is for changing Infuser itself. Game-making agents should use the
[integration instructions](agents.md), their game's existing checks and the
publisher's results. Do not install these development tools or run this test
suite merely to integrate, publish or verify a game.

Use Rust 1.89+ and Node 24. The desktop app is a separate Cargo package; checking
only the root package misses it. Private configs and test artifacts belong in
ignored `.local/` files.

## Format before submitting

Always run rustfmt before committing or submitting changes, including opening or
updating a pull request. This also applies to coding agents; the repository-root
[AGENTS.md](../AGENTS.md) makes the requirement discoverable to them.

From the repository root:

```sh
cargo fmt --all
cargo fmt --manifest-path desktop/Cargo.toml --all
```

Include the formatter's changes in the submission and rerun it after subsequent
Rust edits. Manual formatting and CI checks do not replace running rustfmt.
Verify both packages with the formatting checks below before submitting.

## Fast checks

From the repository root:

```sh
cargo fmt --all -- --check
cargo fmt --manifest-path desktop/Cargo.toml --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --manifest-path desktop/Cargo.toml --locked --all-targets -- -D warnings
cargo test --locked
cargo test --manifest-path desktop/Cargo.toml --locked
node --test tests/*_sdk.mjs tests/themes.mjs tests/voice_chimes.mjs tests/voice_audio.mjs tests/leak_soak.mjs
```

If Node is outside PATH, set `INFUSER_TEST_NODE=/absolute/path/to/node` for the
Rust tests. The compatibility journey executes frozen older SDK files against
the current API. Keep those fixtures unchanged when checking compatibility.

The API journeys use disposable state and local HTTP/TLS servers. They cover
publishing, old SDKs, private groups, chat, metadata, saves, native handoff, peer
signaling, and updater recovery. They do not need real Google accounts or the
production server.

Screenshot coverage: `cargo test --locked --test screenshots_journey` checks
rotation, chat/reaction attachment access and visit boundaries.
`node tests/screenshots_browser.mjs` uses the same `INFUSER_PLAYWRIGHT` and
`INFUSER_CHROMIUM` overrides as the other browser journeys. It checks the real UI
and WebP encoding, then real Chromium tab capture/cropping of a WebGL iframe.
The test browser auto-approves its own fixture tab. Cancellation and unsupported
capture use mocks. Manually choose the game tab to check the OS sharing picker.

For the real private-group browser flow:

```sh
npm ci --prefix services/peerjs
npx --prefix services/peerjs playwright install chromium
INFUSER_GROUP_BROWSER_NODE="$(command -v node)" \
  cargo test --locked --test groups_journey -- --nocapture
```

The test covers inspection, read-only chat, covers, returning home, group creation,
and mobile layout against the real Rust API. Browser setup is a one-time step.
Docker and Caddy journeys are opt-in; see [operations](developer-testing.md#docker-and-caddy).
Engine/hardware acceptance is tracked in the [roadmap](roadmap.md).

The avatar-strip browser journey uses mocked API responses and real browser
navigation (including popup, stale-session, mobile, and keyboard checks):

```sh
INFUSER_PLAYWRIGHT="$PWD/services/peerjs/node_modules/playwright/index.mjs" \
  node tests/friends_journey.mjs
godot --headless --path examples/godot-web --script "$PWD/tests/godot_binary.gd"
```

Desktop tests exercise update integrity, atomic replacement, readiness, failed
startup and timeout handling. Physical Windows acceptance is still required.

## Dependency checks

Run these when changing dependencies or doing a maintenance pass, without adding
a release gate or updating unrelated packages:

```sh
cargo install cargo-machete --locked
cargo install cargo-audit --locked
cargo machete
cargo audit
cargo audit --file desktop/Cargo.lock
npm audit --prefix services/peerjs --omit=dev
```

Read advisories before changing versions. A finding can concern an unused API;
maintenance warnings are separate from known vulnerabilities. Prefer a targeted
update with the relevant journeys over a blanket dependency refresh.

The 2026-11-15 review updated bundled SQLite and ZIP to patched versions. The
remaining RustSec maintenance warning is `ttf-parser` through
`eframe → egui → epaint → ab_glyph → owned_ttf_parser`. Revisit it with an egui
maintenance update; replacing the GUI stack is outside this cleanup.

## Where common behavior lives

- `src/portal/library.rs`: Library fields and group-filtered social data.
- `src/platform/groups.rs`: group ownership and credential-management policy.
- `src/private_file.rs`: private output reservations and durable file replacement.
- `native-protocol`: the shared native-delivery format and path validation.
- `docs/sdk-contract.md`: design review for game-facing compatibility boundaries.

Infuser code is [MIT licensed](../LICENSE). Third-party dependencies and imported
assets keep their own notices and licenses.

## Documentation audiences

Keep each guide for one audience: players, game authors/agents, server operators,
or Infuser developers. Put platform build commands, test harnesses, implementation
notes, and future design work in developer guides. User guides describe available
features and the steps needed to use them. Keep exact API names, explain unfamiliar
terms, and link to the implemented contract rather than a proposal. Start at the
[documentation index](README.md). Feature-specific tests live in
[developer testing](developer-testing.md); storage and compatibility details live
in [implementation notes](implementation-notes.md).

Describe people by their role: user, player, author, admin, or operator. Do not
infer age or gender in documentation, UI copy, agent prompts, or example accounts.

## Unity adapter checks

These checks are for modifying Infuser's adapter. Game authors use their existing
Unity Editor and the [Unity recipe](engines/unity-web.md); they do not install .NET
or run this repository's development harness to publish.

```sh
node --test tests/unity_sdk.mjs
dotnet run --project tests/unity_csharp/UnityChecks.csproj
dotnet run --project tests/unity_csharp/UnityChecks.csproj -p:WebPlayer=true
```

The C# harness uses .NET 10, C# 9, and explicit Unity test doubles, without NuGet
test packages. It compiles the adapter's runtime/Editor code and checks save
retries, conflicts, local validation, typed events, package path resolution,
and export copying. Both Editor and WebGL conditional branches must compile.
It does **not** validate Unity APIs, Unity's serializer, IL2CPP, or browser callbacks.

With Unity 6 and its Test Framework installed, enable the package's Editor tests
by adding `"testables": ["org.infuser.unity-web"]` to the project's package
manifest, preserving existing testables. Run `Infuser.Unity.Tests` in Test Runner.
These tests use real JsonUtility and resolve the installed bridge. The copied
Assets installation also includes the tests, gated by `UNITY_INCLUDE_TESTS`.

Then build [Beacon Garden](../examples/unity-web/README.md) in a separate project.
Verify import and Web export, hosted/standalone startup, cloud save/reload,
conflicts, guest isolation, Join Friend, two-client forced relay, host departure,
and target desktop/mobile browsers. Also test a Unity Web client against the
managed dedicated-server fixture before claiming compiled socket support.
Record actual Editor version, build options, browser/device, and outcomes in
[the Unity review](proposals/unity-agent-experience.md). A C# or JS harness pass
must never be substituted for these acceptance results.

## Library heap and interaction soaks

Use the existing Playwright 1.65.1 installation (or the same overrides as the
Library journeys). No live server, account, browser extensions, or extra leak
framework is needed:

```sh
INFUSER_PLAYWRIGHT="$PWD/services/peerjs/node_modules/playwright/index.mjs" \
INFUSER_CHROMIUM=/path/to/chromium node tests/leak_soak_journey.mjs
```

The fixture intercepts all traffic at `https://library.test`, blocks other
origins and service workers, and serves real `web/` assets. First run the
unchanged-refresh and detail Library journeys; their zero-mutation and 320px
Cartridge Club title/Play alignment checks remain prerequisites.

The default command runs four scenarios, each in a fresh browser context with
its own baseline and snapshot directory. Use `--scenario=polling`, `chat`,
`grid`, or `voice` to isolate one; `--scenario=all` is the default. Each runs
400 measured cycles, with a checkpoint every 50. `--cycles=2000` extends the
measured portion (at least 400, a multiple of 50). Real animations and audio
teardown make the full suite take tens of minutes; use individual scenarios
during iteration. For the original long polling check, use
`--scenario=polling --cycles=2000`.

| Scenario | Fixed warmup | One measured cycle |
| --- | ---: | --- |
| Polling | 20 | Library refresh, client-state check, closed-chat poll |
| Chat | 400 | Scroll one viewport in 60-message history through top/middle/latest and different speakers; close/reopen every ten gestures |
| Grid | 400 | Scroll 40 covered cards middle/bottom/top, open a full card, decode its cover, close |
| Voice | 400 | Join, remote-track subscription/speaker events, mixer, mute/deafen/unmute, output boost/reset, unsubscribe/resubscribe, reconnect events, leave, microphone loopback test, dismiss |

Each interaction scenario first performs 400 unchanged Library/client-state/chat
poll cycles. The real 25-second Library interval alone leaves those background
paths cold during an interaction warmup: snapshots showed `renderGames` and
`renderGameDetail` compiling later, causing false trend flags despite constant
application objects. Both foreground and background paths must be exercised
before the baseline. The listed foreground warmups and background prewarm are
fixed, not an adaptive retry-until-stable loop.
Exploratory chat/grid/voice snapshots showed substantial startup compilation (about
186 KiB, 224 KiB and 288 KiB of additional code respectively), which can trip gates
calibrated for polling even when application objects are stable. The longer
warmups exercise those paths before fixing their baseline; they do not change
the 320 KiB cap or subtract compiled code/instrumentation from any measurement.
Voice also needs the full 400-cycle foreground warmup: a 100-cycle warmup left
`paint`, `stopMicTest` and input-processor code compiling during measurement.
That probe finished just 36,692 heap bytes above baseline (33,776 additional code
bytes), with constant resources and essentially flat arrays/objects, but still
triggered the sustained-growth rule. The longer fixed warmup addresses that
observed startup work rather than widening either heap gate.
The [2,000-cycle grid calibration](../tests/fixtures/leak-grid-calibration.json)
records the exploratory data separately from acceptance runs. Its final three
readings rose by just 160 bytes total and would fail the old rule; all resource
counts stayed fixed. Rebasing that recorded series after the fixed grid warmup
passes every unchanged absolute/resource/sustained-growth gate through cycle
2,000. A regression check preserves that evidence.

Polling cycles 101–150 simulate a hidden document through visibility getters
and its event, without changing clocks. Chat has zero live poll timers while
hidden and exactly one again on return; Library and client-state keep one each.
Configured voice keeps exactly one status interval. Real intervals remain
running in every scenario. Before checking timer counts, the harness drains
already-running polls so an in-flight self-scheduling chat poll is not mistaken
for a missing timer.

Chat asserts that history really scrolls and speaker movement really runs; the
fixture includes speakers outside the connecting avatar slots. Grid asserts
scrolling, detail open/close, and cover decoding. Voice uses Chromium's synthetic
microphone and real MediaStream tracks, AudioContexts and Infuser audio processors.
Only the LiveKit room/transport boundary and API are mocked; no voice server or
real microphone is contacted. It verifies tracks, contexts and the long-poll
request are active while joined and all gone after leaving. Voice measurements
reconcile the off/empty roster and render the same dismissed mixer state each
time. This does **not** exercise LiveKit's transport internals, ICE/TURN, codecs,
real device drivers or multi-client delivery; the real-media voice journey
remains separate.

Work is awaited through promises, actual scroll/render events, finite animation
completion and audio-context closure. The voice chime's real delayed close runs
normally. A 15-second interaction deadline rejects a hung cycle; it does not
sleep, retry an action, clean up application resources, or advance clocks. Live
animation-frame callbacks must return to zero at rest. Test-served module hooks
only expose/observe existing functions and live handles.

Every measurement collects garbage three times through CDP, then reads
`Performance.getMetrics`. Nodes, event listeners, documents and frames must equal
the scenario baseline exactly. Post-GC JS heap must stay within a fixed **320 KiB**
of baseline. Starting at cycle 400, every checkpoint also checks the last eight
measurements: split them into four consecutive pairs (100 cycles each), take
each pair's minimum, and fail if all three increases between these low-water
marks are at least **8 KiB**. This catches sustained growth with intervening dips,
below the absolute cap. It replaces the original final-three-readings rule:
small positive steps alone do not establish a leak. There are no sleeps, fake
clocks, `performance.memory` readings, percentage tolerances, or retries.
Very small leaks can still evade a finite run.

Run `node --test tests/leak_soak.mjs` for diagnostic/gate regression checks,
including sawtooth growth, bounded variation, an inspector table larger than a
leaked array, growing existing arrays, alternate/weak retaining paths, and
compiled-code reporting. These fast helper checks also run in the existing PR
Node job; the browser soak remains an explicit local run.

The one-time 2,000-cycle run on 2026-11-24 used Chromium 156.0.8078.79,
Playwright 1.65.1 and Node 24.22.1. Its raw measurements are recorded in
[`tests/fixtures/leak-soak-2000.json`](../tests/fixtures/leak-soak-2000.json),
not used as expected heap values for future browsers. Heap started at 2,742,140
bytes, peaked at 2,959,068 (+216,928 bytes, about 212 KiB), and finished at
2,951,276. Growth settled rather than scaling with the cycle count; the final
500-cycle range was 4,444 bytes. All resource counts stayed constant. A 320 KiB
cap leaves 110,752 bytes (about 108 KiB) above that observed peak, substantially
less than the original guessed 1 MiB. The 8 KiB block step exceeds the entire
observed final 500-cycle range; the recorded series passes every rolling trend
check. These are empirical bounds for this fixture, not a guarantee for every
browser or a proof that all application paths are leak-free. Revisit them with
recorded evidence if the browser or fixture changes, rather than relaxing a
failed assertion automatically.

Each checkpoint writes a heap snapshot under `.local/leak-soak/<scenario>/run-*`. A passing
run also writes `results.json` with scenario/warmup/cycle metadata, the raw table and threshold. Failures capture without awaiting potentially hung work and write
`failure.heapsnapshot` and `failure.json`, print the checkpoint table and
snapshot self-size deltas by node type (including compiled code), constructor count **or size** growth (including growing existing arrays), and
show representative strong GC-root paths for up to eight growing constructor
and storage groups per category. Groups rank by total new/expanded self bytes,
not just one large allocation. These are investigation candidates, not proven
culprits; replacement allocations can contribute to that total.

Known Blink DevTools, Inspector and NetworkResourcesData retainers, and the
`__leakSoak` test harness, have a separate instrumentation section. Paths that
avoid instrumentation take precedence if both paths exist. Unknown retainers
remain candidates; this is not a complete engine ownership classifier. Weak
references are never used as strong-path evidence. **Nothing is subtracted from
any metric gate**, including inspector bookkeeping. The complete snapshots
remain available to investigate other nodes beyond the printed shortlist.

These artifacts are ignored by Git. Player iframes, real voice transport,
mobile/touch soak variants, and a scheduled workflow remain separate work.

An initial chat probe (open/scroll/close on every cycle) exposed a real leak: 50 cycles after warmup retained
29,150 additional DOM nodes, 1,200 listeners, and 1,579,908 heap bytes. Menu
callbacks captured the old `renderPresence` scope, including earlier layout
maps/arrays; retained menus linked those scopes into a growing chain. Constructing
menu callbacks in a separate helper keeps them scoped to their own menu instead.
The first 50-cycle checkpoint with that fix held nodes/listeners at 4,970/1,707
and grew only 63,300 bytes. The regression exercises the real speaker animations
and checks the same strict resource equality throughout the measured run.

The final 400-cycle acceptance runs on the same Chromium/Playwright setup passed
all gates with these peak increases above their fixed post-warmup baselines:

| Scenario | Peak heap increase | Constant nodes / listeners |
| --- | ---: | ---: |
| Polling | 116,040 bytes | 4,068 / 1,226 |
| Chat | 0 bytes | 4,970 / 1,707 |
| Grid | 42,152 bytes | 4,173 / 1,362 |
| Voice | 98,892 bytes | 4,150 / 1,244 |

Documents and frames stayed at one in every run. These are observed results,
not expected heap values for later runs or proof that untested paths cannot leak.
