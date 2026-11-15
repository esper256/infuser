# Godot web: export, upload, play

This is the second engine study. The publisher understands Godot's export layout;
the full Godot save-bridge UX still needs a real Godot game journey.

For an existing project with a **Web** export preset and matching Godot export
templates installed on the game-making machine:

```sh
infuser publish /private/game.credentials.json . --solo --title "My Game"
```

The CLI detects `project.godot`, finds the Web preset, runs a headless release
export into a temporary directory, and uploads it. Use `--godot godot4` if that
is your executable's name. It does not need Godot on the Infuser server.

For an already-exported game:

```sh
infuser publish /private/game.credentials.json /path/to/export --solo
```

Keep `index.html`, `.pck`, `.wasm`, JS, and other exported files together.
Root-relative paths work. `.pck` exports automatically enable COOP/COEP for
threaded exports; set `cross_origin_isolation` explicitly in `infuser.game.json`
if the export needs different behavior. Missing WASM/assets return 404. Large
assets support HTTP range requests. Current limits: 64 MiB/file, 256 MiB/build.

Do not rewrite a native Godot game around JavaScript. Add a small web save bridge
at the game's existing load/checkpoint boundary using Godot's `JavaScriptBridge`
and the browser SDK's `ready`/`saves` methods. Preserve callbacks until completion,
await load before starting gameplay, and show save failures. Guest mode's separate
host isolates Godot's browser filesystem, and cloud saves use a separate guest
namespace. Local `user://` persistence alone is not cloud saving.

If the game requires unsupported web features, report the limitation explicitly.
Native client download/launch integration is not part of this first web slice.
