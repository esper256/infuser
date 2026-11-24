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
node --test tests/*_sdk.mjs tests/themes.mjs tests/voice_chimes.mjs tests/voice_audio.mjs
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

## Library heap soak

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

After 20 warmup cycles, it fixes a post-GC baseline and invokes the production
Library refresh, client-state check and closed-chat poll functions 200 times
each. Checkpoints occur every 50 cycles. Cycles 101–150 simulate a hidden
document through visibility getters and the visibility event, without changing
clocks. Chat intentionally has zero live poll timers while hidden and exactly
one again on return; Library and client-state keep one each throughout. Real
intervals remain running. Test-served module instrumentation only exposes and
observes the existing functions and live timer handles; it does not replace
polling logic, cache history, retry work, or wait with sleeps.

Every measurement collects garbage three times through CDP, then reads
`Performance.getMetrics`. Nodes, event listeners, documents and frames must equal
the baseline exactly. Post-GC JS heap must stay within a fixed 1 MiB of baseline,
and the last three checkpoints cannot be strictly increasing. No
`performance.memory` or percentage tolerance is used as an oracle.

Each checkpoint writes a heap snapshot under `.local/leak-soak/run-*`. Failures
also write `failure.heapsnapshot` and `failure.json`, print the checkpoint table,
constructor counts that grew, and a strong-reference path from a GC root to the
node with the largest increase in self size (including array backing storage).
These artifacts are ignored by Git. The first slice covers idle polling only;
player iframes, voice sessions, and a scheduled workflow are separate work.
