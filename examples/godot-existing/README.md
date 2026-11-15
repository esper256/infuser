# Existing-project integration study

Use the official **Dodge the Creeps** GDScript project, pinned to
[`b761b4cd5718a7118f2c95eb55f181d0a5f004f1`](https://github.com/godotengine/godot-demo-projects/tree/b761b4cd5718a7118f2c95eb55f181d0a5f004f1/2d/dodge_the_creeps).
Its existing title, scenes, controls, spawning, scoring and art remain intact.
Retain its LICENSE and the music/font/image notices in its README when distributing it.
This directory contains only our integration overlay, not a fork of the upstream game.

Apply the [Godot web agent recipe](../../docs/engines/godot-web.md):

1. Copy the existing project into a scratch checkout. Copy
   `examples/godot-web/addons/infuser` into its `addons/infuser` directory.
2. Add autoloads `Infuser="*res://addons/infuser/infuser.gd"` then
   `InfuserProgress="*res://infuser_record.gd"`. Copy `record.gd` from this
   directory to `infuser_record.gd` and copy `infuser.game.json` beside the project.
3. At the end of its existing `game_over()` function, add
   `InfuserProgress.save_score(score)`. Preserve the original gameplay code.
4. Add a Compatibility Web export preset with
   `include_filter="addons/infuser/*.js"`, and exclude credentials and build output.
   Export normally and publish with the project's private Infuser connection.

This trial adds a declared best-score checkpoint at the existing game-over
boundary. The integration loads before enabling Start, handles guest namespaces,
offers the shared conflict chooser, and retains failed writes for an explicit
idempotent retry. It does not pretend local `user://` files are automatically
cloud-synchronized. Its read-only `window.dodgeProbe` is acceptance instrumentation,
not part of the SDK.

Run the local authenticated fixture with `--project PATH_TO_ADAPTED_PROJECT`.
The fixture's app ID remains `lantern-lake`; its title and save declaration come
from the supplied existing project. No real account, public game listing, or
upstream repository change is required for this study.
