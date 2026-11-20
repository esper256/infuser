# SDK compatibility foundation — preview

Audience: game authors and their agents.

SDK major 1 and game API major 1 now have independent, explicit entry points.
**This is a 1.0 candidate, not a stable 1.0 release.** Check the installation's
advertised capabilities before using a feature. Manifest schema 1 describes the
manifest document format, independently of SDK versions.

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

## Required and optional features

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
                     "runtime_sessions.v1", "presence.v1", "launch_intents.v1",
                     "sessions.v1", "connections.websocket.v1"],
    "legacy_path": "/_infuser/sdk.js"
  }
}
```

For a peer-hosted game with independent rooms, require `peers.rooms.v1` plus
the adapter capability (`peers.webrtc.v1` for the built-in transport or
`peers.peerjs.v1` for PeerJS). All require TURN. Peer-hosted WebRTC rooms admit
2–8 players, including the host. `multiplayer.max_players` chooses the cap
inside that range. A full room reports reason `full`, and a leave frees the
seat. PeerJS also requires the private PeerServer service. Older servers may
advertise WebRTC without independent rooms.
The same SDK advertisement appears in `session.capabilities.sdk`. Capabilities describe API
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
`/presence/v1`, `/launch-intent/v1/consume`, `/connections/v1/...`, and `/peers/...`. Feature-specific
`v1` components remain explicit; they are independent of SDK packaging.
Authentication, origins, save ownership, guest isolation, readiness and error
semantics are unchanged. Context, declared checkpoints, runtime sessions,
presence and launch intents retain their existing response shapes.
