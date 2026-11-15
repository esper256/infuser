# SDK compatibility foundation — preview

SDK major 1 and game API major 1 now have independent, explicit entry points.
**This is a 1.0 candidate, not a stable 1.0 release.** Review identity, saves,
sessions and multiplayer contracts before freezing them. Manifest schema 1 is
still just the manifest document format.

For a new Three.js/Vite integration, put this in `infuser.game.json`:

```json
{
  "sdk": {
    "major": 1,
    "api_major": 1,
    "required_capabilities": ["identity.v1", "saves.v1"]
  }
}
```

Import at runtime; the CLI includes the same selected SDK once in the uploaded
HTML for automatic session renewal:

```js
const url = new URL('/_infuser/sdk/v1.js', location.origin).href;
const infuser = await import(/* @vite-ignore */ url);
const session = await infuser.ready;
console.log(infuser.version); // {major: 1, apiMajor: 1, status: 'preview'}
```

The CLI does not rewrite a game's bundled JavaScript. When opting an existing
project into major 1, update its runtime import and remove its old SDK script
from the source HTML. Do not load two SDK generations into the same document.
Existing projects with no `sdk` declaration keep the legacy injected script.

## Admission and optional features

`GET /api/publisher/capabilities` includes:

```json
{
  "sdk": {
    "status": "preview",
    "majors": [1],
    "api_majors": [1],
    "default_major": 1,
    "path": "/_infuser/sdk/v1.js",
    "api_base": "/_infuser/api/v1",
    "capabilities": ["identity.v1", "saves.v1", "cloud_checkpoints.v1",
                     "runtime_sessions.v1", "presence.v1", "launch_intents.v1"],
    "legacy_path": "/_infuser/sdk.js"
  }
}
```

`peers.webrtc.v1` is also present when TURN is configured. The same SDK
advertisement appears in `session.capabilities.sdk`. Capabilities describe API
availability; the game's existing topology, save-group and launch-intent
manifest declarations still determine which features it can use.

Declare only capabilities without which the game cannot work. For optional
features, inspect capabilities and keep the game playable if absent. A missing
optional feature is not a publish/launch error. Unknown required capability,
unsupported SDK major or unsupported API major is rejected with HTTP 422 before
accepting a build, and checked again at promotion and launch. Example:

```json
{"error":"required capability unavailable: peers.webrtc.v1"}
```

There is no automatic major substitution. Unsupported versioned routes return
404 rather than serving a newer SDK or the game's SPA shell. No code executes on
the server to detect features: agents declare what their game needs.

## Wire routing

The v1 SDK uses `/_infuser/api/v1` followed by the existing feature route:
`/context`, `/heartbeat`, `/saves/SLOT`, `/cloud/v1/groups/GROUP`, `/runtime/v1`,
`/presence/v1`, `/launch-intent/v1/consume`, and `/peers/...`. Feature-specific
`v1` components remain explicit; they are independent of SDK packaging.
Authentication, origins, save ownership, guest isolation, readiness and error
semantics are unchanged. Context, declared checkpoints, runtime sessions,
presence and launch intents retain their existing response shapes.

Major 1 modules live under `sdk/v1/`; their HTTP route adapter is
`src/portal/sdk_v1.rs`. Legacy imports and wire routes remain served separately.
They share handlers where behavior matches today. A future incompatible major
must add a new adapter and split changed handlers, never redirect old callers
to the new behavior. Compatible fixes can update a major's files, so responses
revalidate rather than claiming immutable JavaScript bytes.

## Regression and rollback

The frozen `tests/fixtures/legacy-sdk` snapshot is an old client, not another
implementation to keep editing. A real HTTP journey executes it unchanged
against the current daemon, covering launch, identity, save/reload, guest refresh,
guest isolation and renewal, then verifies shared save ownership with major 1.
Both served SDK generations also run the fast cloud/recovery checks.

Run `cargo test --locked` with Node 24 on PATH, or set `INFUSER_TEST_NODE` to its
absolute executable path. `node --test tests/*_sdk.mjs` runs the clock-controlled
SDK checks. Node is a development/test dependency, not a controller dependency.

Declarations are stored in an additive SQLite side table so an older catalog
writer cannot erase them. This does not make a daemon predating versioned routes
capable of serving major 1. Roll back to a release supporting every deployed SDK
major; older games retain their unversioned path. No save schema is migrated by
opting a game into this SDK major.
