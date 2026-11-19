# Godot web: export, upload, play

Audience: game authors and their agents.

**Creating a game?** Read this recipe for engine constraints, then return to the
[creation guide](../create-game.md). Skip connection and publishing commands for now.
**Integrating an existing game?** Start with the [publishing checklist](../agents.md),
connect once, then apply this recipe. Return to
[Explicit play modes](../agents.md#explicit-play-modes) and finish that checklist's
saves, artwork, publishing, and handoff steps.

This preview supports Godot 4.7.2 standard/GDScript with the Compatibility
renderer, using single-threaded or threaded web exports. Physical laptop sleep
and Windows browser testing remain unverified. The [example project](../../examples/godot-web/README.md)
shows saves, guest play, and two-player multiplayer.

For an existing project with a **Web** export preset and matching Godot export
templates installed on the game-making machine:

```sh
infuser publish /private/game.credentials.json .
```

The CLI detects `project.godot`, finds the Web preset, runs a headless release
export into a temporary directory, and uploads it. Use `--godot godot4` if that
is your executable's name. It does not need Godot on the Infuser server.

First put the actual play modes, SDK requirement, and save declarations in
`infuser.game.json` at the project root, following the linked example. Use
`--solo` only for genuinely single-player games.

For an already-exported game, still pass the project root so declarations and
artwork are included:

```sh
infuser publish /private/game.credentials.json /path/to/project --web-dir /path/to/export
```

Keep `index.html`, `.pck`, `.wasm`, JS, and other exported files together.
Root-relative paths work. `.pck` exports automatically enable COOP/COEP for
threaded exports; set `cross_origin_isolation` explicitly in `infuser.game.json`
if the export needs different behavior. Missing WASM/assets return 404. Large
assets support HTTP range requests. Current limits: 64 MiB/file, 256 MiB/build.

Declare SDK major 1 as in the [example manifest](../../examples/godot-web/infuser.game.json).
Copy [addons/infuser](../../examples/godot-web/addons/infuser), put the `Infuser`
autoload before game autoloads, and include
`addons/infuser/*.js` in the Web preset's non-resource include filter. The bridge
is packaged in the PCK and imports `/_infuser/sdk/v1.js` on the game's origin.
It retains callbacks and returns explicit results. Connect it at the game's
existing load/checkpoint boundary; keep simulation and multiplayer in GDScript.

Use `call_api("load", {"group": ...})` and `call_api("commit", {"group": ...,
"checkpoint": ...})`; see the example for these preview shapes. Checkpoints
include `expectedRevision`, a stable `commitId` and the complete declared file
set. Preserve the same payload/ID on retry. A conflict preserves both candidates
and must not be called "Saved". `save_conflict.gd` supplies a reusable chooser:
show both checkpoint summaries/timestamps, validate each copy using the game's
schema, and explicitly choose one. Both remain preserved until resolution succeeds.
Cancel leaves the conflict pending; retry uses the same resolution commit ID.

For an existing-project walkthrough, see [Dodge the Creeps](../../examples/godot-existing/README.md).
It adds a best-score checkpoint at game-over while keeping the original gameplay.

For co-op, copy the current peer addon and require `peers.rooms.v1`. Use
`await peers.create()` for Play and an unchecked Play Solo checkbox for local
play. Each host gets a separate room. A host that needs setup time can start
closed with `await peers.create(false, false)`.
Call `await peers.set_joinable(true)` when friends can enter and
`await peers.set_joinable(false)` to close admission. The default is open.

Declare `launch_intents: ["join_friend"]` for supported peer-hosted games. Consume
the intent once at startup and pass it to the Godot peer adapter as
`peers.join(false, intent)`; this is GDScript, not the JavaScript SDK signature. A stale
target must return to the title screen, never retry as a new host. Ordinary Play
still opens the game's normal title screen.

Guest mode's separate host isolates Godot's browser filesystem; cloud saves have
a separate guest namespace. Local `user://` persistence alone is not cloud saving.
This adapter supports declared JSON and binary checkpoints. Use `Infuser.encode_binary`
and `Infuser.decode_binary` for bytes; see [binary saves](../cloud-save-design.md#binary-files).
It does not scan or automatically sync `user://`.

If the game requires unsupported web features, report the limitation explicitly.
Native delivery has a separate [desktop preview recipe](godot-native.md).
