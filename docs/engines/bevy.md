# Bevy: not yet validated

Infuser does not yet provide a tested Bevy adapter or an end-to-end Bevy publishing
recipe. A WASM export alone does not verify identity, cloud saves, guest isolation,
or multiplayer integration. Native Bevy delivery is also unsupported.

Keep an existing game's renderer and networking intact. Report this integration
gap using the [agent guide](../agents.md#reporting-platform-problems), rather than
presenting an untested export as supported. Bevy work is paused while the current
browser and Godot contracts settle.
