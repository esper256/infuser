# Bevy: not yet validated

Audience: game authors and their agents.

For a new game, compare supported options in the [creation guide](../create-game.md).
For an existing game, return to the [publishing checklist](../agents.md) after
assessing the limits below.

Infuser does not yet provide a tested Bevy adapter or an end-to-end Bevy publishing
recipe. A WASM export alone does not verify identity, cloud saves, guest isolation,
or multiplayer integration. Native Bevy delivery is also unsupported.

Keep an existing game's renderer and networking intact. Report this integration
gap using the [agent guide](../agents.md#reporting-platform-problems), rather than
presenting an untested export as supported.
