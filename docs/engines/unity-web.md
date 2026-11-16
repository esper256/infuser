# Unity Web: export, upload, play

Audience: game authors and their agents.

Use this recipe for [Choose the integration](../agents.md#choose-the-integration).
Complete the main guide's connection step first. Apply the sections your game
needs, then return to [Explicit play modes](../agents.md#explicit-play-modes)
and finish its artwork, publishing, and handoff steps.

**Experimental.** Infuser can host Unity Web exports, including gzip, Brotli,
and decompression-fallback files. The C# adapter targets Unity 6 and has not yet
been compiled with a Unity Editor. Its JavaScript bridge and platform behavior
have automated tests. Do not describe a source-only check as a Unity playtest.

## Export an existing game

Use the project's existing Unity Editor and **Web Build Support** module to make
a Web build. Keep its `index.html`, `Build`, `TemplateData`, `StreamingAssets`,
and any other exported files together. Do not upload `Assets`, `Library`, or
`ProjectSettings`. Infuser needs neither Unity nor Docker on the publishing machine
to upload an already-built export.

Put `infuser.game.json` at the project root. For a single-player game:

```json
{
  "title": "Your existing game name",
  "web_dir": "Builds/Web",
  "sdk": {"major": 1, "api_major": 1, "required_capabilities": ["identity.v1"]},
  "play_modes": ["solo"],
  "solo_runtime": "client_only",
  "cross_origin_isolation": false
}
```

Use the [publisher download](../publisher-install.md) or [HTTP recipe](../publishing-http.md).
Publish the project root so its declarations and artwork are included:

```sh
infuser publish /private/game.credentials.json /path/to/project --preview
```

`web_dir` is relative to the project. `--web-dir /absolute/export/path` overrides
it. The publisher uploads that export and injects the versioned browser SDK into
the uploaded HTML; it leaves your local export unchanged. Unity source projects
without an explicit export path are rejected with instructions.

Gzip or Brotli with **Decompression Fallback off** uses browser decompression.
Fallback exports ending in `.unityweb` also work: Unity's loader handles them.
Keep the filenames produced by Unity. If the export requires shared memory,
set `cross_origin_isolation: true`; third-party resources must then satisfy the
browser's isolation rules. This flag does not add thread support to Unity code.
Current limits are **64 MiB per file and 256 MiB per build**. Read the server's
publishing capabilities for its limits. Reduce textures/audio or split supported
content bundles if necessary; report a size blocker rather than silently dropping assets.

## Add identity, cloud saves, or multiplayer

Copy the complete [Assets/Infuser folder](../../sdk/unity/Assets/Infuser) into
your project's `Assets/Infuser`. Keep that path. There are no extra Unity packages.
Rebuild: the included Editor hook places `infuser-unity.mjs` beside `index.html`.
Keep that file in the upload. No custom Web template is required.

```csharp
using Infuser.Unity;

var client = InfuserClient.Instance;
var identity = await client.ReadyAsync();
if (identity.hosted) {
    // identity.name, player_id, guest. Load cloud progress before gameplay.
} else {
    // Existing standalone/Editor behavior, using its own local saves.
}
```

Call from Unity's main thread. The receiver survives scene changes. Catch errors
and offer a short retry or return-to-Library action. Hosted authentication errors
remain errors: **never switch to local owner saves or public matchmaking after a
failed hosted connection**. Outside Infuser, `hosted` is false and the adapter
does not contact Infuser. Keep the game's existing standalone implementation.
The adapter does not prompt for Google login or handle publisher credentials.

## Save at explicit checkpoints

Add `cloud_checkpoints.v1` to `required_capabilities` and declare your files:

```json
"cloud_saves": {
  "groups": [{"id": "progress", "label": "Progress",
    "files": [{"path": "progress.json", "format": "json"}]}]
}
```

For one JSON file per group, use a `[Serializable]` class with public fields:

```csharp
[System.Serializable]
public class Progress { public int schema = 1; public int level; }

var save = new JsonSave<Progress>(client, "progress", "progress.json");
await save.LoadAsync();
var progress = save.Value ?? new Progress();
// Validate schema/ranges, then start gameplay.
var result = await save.SaveAsync(progress, "Level " + progress.level);
// Only result.status == "saved" means saved.
```

`JsonSave` retains the exact serialized checkpoint and commit ID after a failed
request. Use `RetryAsync()` before creating another checkpoint. A conflict keeps
both copies: read `ReadConflictAsync(false)` (cloud) and `ReadConflictAsync(true)`
(this session), show summaries, validate the chosen copy, then call
`ResolveAsync(useDevice)`. Another concurrent write may require another choice.
Do not silently overwrite either copy. The [reference project](../../examples/unity-web/README.md)
demonstrates the chooser.

Pending data lives in memory, not durable offline storage. Keep the page open
while retrying. Larger games can use `CallJsonAsync("cloud.load", ...)`,
`cloud.commit`, `cloud.read`, `cloud.history`, and `cloud.resolve` with the existing
[checkpoint shapes](../cloud-save-design.md); binary files use `{ "base64": "…" }`.
Each checkpoint contains the complete declared group. `JsonUtility` does not
support dictionaries: use the single-file helper or your project's JSON library
for multi-file groups. Do not add a new serializer merely to save one file.

Infuser scopes cloud data to the player, game, and visit. Guest launches get fresh
isolated cloud data and browser origins. Never copy signed-in progress into guest
mode. Unity's `PlayerPrefs` and `persistentDataPath` use browser storage; they are
not cloud saves. The adapter does not scan or sync them automatically. Updates
can change the browser origin: hydrate from cloud rather than trusting local data.

## Multiplayer and Join Friend

For a **new small co-op, arena, board, or party game**, the simplest integration
is the browser SDK's WebRTC JSON transport through C#. No game server is needed.
Use the [peer manifest](../peer-hosting.md), including both modes if applicable,
`peers.webrtc.v1`, mandatory relay fallback, and `launch_intents: ["join_friend"]`.

```csharp
client.Event += (kind, json) => { /* peer_state, peer_message, peer_end */ };
// Consume once after loading progress; ordinary Play has no intent.
var launch = await client.ConsumeLaunchIntentAsync();
// On a multiplayer selection, or when launch.intent?.kind == "join_friend":
var room = await client.JoinPeersAsync(new PeerOptions {
    intent = launch.intent, relayOnly = false, joinable = false
});
if (room.role == "host") await client.SetJoinableAsync(true);
// SendPeerAsync(hostId, input), BroadcastAsync(snapshot), LeaveMultiplayerAsync().
```

Ordinary Play opens the title screen. Do not join multiplayer just because the
SDK is ready. Failed Join Friend must return to that screen; never remove its
target and create a replacement room. `PresenceAsync()` returns the existing
[presence JSON](../presence.md), including who is online and joinable sessions.

`peer_message` contains `{from, json}`; parse the inner JSON into the game's message
type. The host validates inputs and owns shared simulation. `SendPeerAsync`
reports `sent:false` under congestion; send the newest state later instead of
queueing every missed frame. Messages are ordered/reliable JSON, bounded by the
browser peer SDK. Host departure ends the session; there is no host migration,
automatic transform replication, rollback netcode, or arbitrary Unity RPC support.
Use `relayOnly: true` for a relay test, normal ICE selection for everyday play.

For **persistent worlds or authoritative multiplayer**, use a managed dedicated
WebSocket server. The adapter's `socket.connect` operation takes the same options
as [`sessions.connect`](../dedicated-sessions.md), including an exact Join Friend
intent. `socket.send` accepts `{text}` or `{base64}` and returns `{sent}`;
`socket_message` events carry the same shapes. `network.close` leaves the session.
Server-side room reports, identity admission, and save ownership still follow
the dedicated-session contract. [Node source packaging](node-server.md) is one
available backend; custom containers must run on the host's architecture.

For an **existing Netcode, Mirror, Fusion, or other networking stack**, inspect
its Web transport first. These bridges are not drop-in implementations of those
packages. Browser clients cannot use raw TCP/UDP. Preserve existing gameplay and
explain any necessary transport change before rewriting networking. Do not hide
multiplayer behind a single-player declaration or assume a native Unity server
will run on an ARM64 host. Native Unity delivery is outside this recipe.

## Verify and hand back

Check that the export and `infuser-unity.mjs` exist, publish a preview, and inspect
the publisher's result and server diagnostics. If you have a Unity Editor, check
its build log. Without authenticated play access, report that limit and invite
the user to press Play in their Library; **do not request Google credentials or
pause waiting for their agent to log in**. Follow the main guide's art prompt
option when high-quality image generation is unavailable.

Unity references: [Web deployment and compression](https://docs.unity.com/en-us/engine/6000.3/manual/platform-specific/webgl/building-distribution/deploying),
[browser plug-ins](https://docs.unity.com/en-us/engine/6000.3/manual/platform-specific/webgl/develop/interactingwithbrowserscripting/web-interacting-browser-js),
[Web networking](https://docs.unity.com/en-us/engine/6000.0/manual/platform-specific/webgl/develop/networking).
