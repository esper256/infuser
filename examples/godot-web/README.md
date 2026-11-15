# Lantern Lake: Godot web dogfood

A disposable two-player game exercising actual Godot WASM, GDScript checkpoint
code and `WebRTCMultiplayerPeer` RPCs. No external art dependencies. Keep it hidden
with `--preview` until explicitly choosing to list it.

Requires Godot **4.7.2 standard**, its matching web export templates, and the
Infuser CLI. The tested preset uses GDScript, Compatibility rendering, no threads,
and no service worker. No game server containers are needed.

```sh
godot --headless --path examples/godot-web --editor --import
mkdir -p examples/godot-web/export
godot --headless --path examples/godot-web --export-release Web export/index.html
infuser publish /private/lantern-lake.credentials.json examples/godot-web --web-dir export --preview
```

The publisher also detects the project and runs the Web preset if you omit
`--web-dir`; `--godot` selects a particular executable. Generated `export/` and
`.godot/` contents are ignored. Keep credentials outside the project. Do not use
`--solo`: this manifest supports both modes.

Arrow keys drift; **C** collects a lantern, **S** saves, **M** joins multiplayer,
and **L** leaves. Buttons do the same. Normal Play opens without creating a room.
`?autoplay=1&multiplayer=1&relay=1` exercises forced TURN with deterministic
movement. Omit `relay=1` to allow direct paths.

The copyable `addons/infuser` folder is a **web-only preview**, not a stable SDK.
Keep the `Infuser` autoload and the `.js` non-resource include filter when copying.
The JS bridge handles HTTP/cookies and existing cloud helpers; Godot owns peer
connections, its high-level multiplayer channels and game RPCs. The read-only
`window.lanternLake` acceptance probe contains no credentials.

## Opt-in acceptance journeys

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
acceptance remains pending. See the [engine study](../../docs/engines/godot-web.md)
and [native preview](../../docs/engines/godot-native.md).
