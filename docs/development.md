# Development checks

Use Rust 1.89+ and Node 24. The desktop app is a separate Cargo package; checking
only the root package misses it. Private configs and test artifacts belong in
ignored `.local/` files.

## Fast checks

From the repository root:

```sh
cargo fmt --check
cargo fmt --manifest-path desktop/Cargo.toml --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --manifest-path desktop/Cargo.toml --locked --all-targets -- -D warnings
cargo test --locked
cargo test --manifest-path desktop/Cargo.toml --locked
node --test tests/*_sdk.mjs tests/themes.mjs
```

If Node is outside PATH, set `INFUSER_TEST_NODE=/absolute/path/to/node` for the
Rust tests. The compatibility journey executes frozen older SDK files against
the current API. Keep those fixtures unchanged when checking compatibility.

The API journeys use disposable state and local HTTP/TLS servers. They cover
publishing, old SDKs, private groups, chat, metadata, saves, native handoff, peer
signaling, and updater recovery. They do not need real Google accounts or the
production server.

For the real private-group browser flow:

```sh
npm ci --prefix services/peerjs
npx --prefix services/peerjs playwright install chromium
INFUSER_GROUP_BROWSER_NODE="$(command -v node)" \
  cargo test --locked --test groups_journey -- --nocapture
```

The test covers inspection, read-only chat, covers, returning home, group creation,
and mobile layout against the real Rust API. Browser setup is a one-time step.
Docker and Caddy journeys are opt-in; see [operations](operations.md#verification).
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
- `docs/sdk-contract.md`: game-facing compatibility boundaries.

Infuser code is [MIT licensed](../LICENSE). Third-party dependencies and imported
assets keep their own notices and licenses.
