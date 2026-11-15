# Godot web: export, upload, play

This adapter is a preview. The real Godot/WASM
[Lantern Lake journey](../../examples/godot-web/README.md) covers cloud saves,
guest refresh/isolation, conflicts, save retries, and forced-relay multiplayer.
Tested: Godot 4.7.2 standard/GDScript, Compatibility renderer, both single-threaded
and threaded web exports. Bazzite's installed Flatpak Chrome passed real Godot
RPCs through TURN, brief outages, connection replacement, background operation,
credential refresh with a transient failure, and renderer suspension beyond the
room lease. Tests used headless Chrome; physical laptop sleep and actual Windows
browsers remain acceptance items. This is not an SDK 1.0 declaration.

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
The study retained its title, scenes, assets and gameplay, adding a best-score
checkpoint at game-over. Real gameplay, save/reload and guest isolation passed.

Declare `launch_intents: ["join_friend"]` for supported peer-hosted games. Consume
the intent once at startup and pass it to the Godot peer adapter as
`peers.join(false, intent)`; this is GDScript, not the JavaScript SDK signature. A stale
target must return to the title screen, never retry as a new host. Ordinary Play
still opens the game's normal title screen.

Guest mode's separate host isolates Godot's browser filesystem; cloud saves have
a separate guest namespace. Local `user://` persistence alone is not cloud saving.
This adapter stores JSON through the cloud API; it does not automatically sync
arbitrary binary save files or scan `user://`.

If the game requires unsupported web features, report the limitation explicitly.
Native delivery has a separate [desktop preview recipe](godot-native.md).
