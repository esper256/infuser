# Lantern Lake: Godot web example

Audience: game authors and their agents.

`--preview` in this fixture is for admin-only testing. Publish normally when an
ordinary user needs to play; hidden builds are not available to them.

A small two-player game showing Godot web exports, GDScript cloud saves,
and `WebRTCMultiplayerPeer` RPCs. No external art dependencies. Keep it hidden
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

Arrow keys drift; **C** collects a lantern, **S** saves, **M** presses Play,
and **L** leaves. The Library opens the game without creating a room. The game's Play button hosts a fresh room unless Play Solo is checked. Friends join through the Library.
`?autoplay=1&multiplayer=1&relay=1` exercises forced TURN with deterministic
movement. Add `matchmaking=1` on both test clients to exercise legacy automatic matchmaking. Omit `relay=1` to allow direct paths.

The copyable `addons/infuser` folder is a **preview**, not a stable SDK.
Its cloud-save adapter also supports [native Godot](../../docs/engines/godot-native.md);
multiplayer is web-only.
Keep the `Infuser` autoload and the `.js` non-resource include filter when copying.
The JS bridge handles HTTP/cookies and existing cloud helpers; Godot owns peer
connections, its high-level multiplayer channels and game RPCs.
