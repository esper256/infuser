# Put an existing game on Infuser

Keep the game's name, engine, gameplay, and build workflow. The child supplies the
Add a game prompt; you handle integration and publishing. They should not need to
configure hosting, manage credentials, or create a GitHub account.

## Connect once

1. Look for this game's existing Infuser connection. Reuse its credentials and app
   ID on the target server, even if the prompt reserves a different ID. Do not
   create a duplicate game.
2. Otherwise, exchange the prompt's setup code **before lengthy code changes**.
   Codes last three hours from creation and work once. Use a [prebuilt publisher](publisher-install.md)
   on macOS or Linux, or the [HTTP recipe](publishing-http.md) with existing tools
   on macOS, Windows, or Linux. Rust and Docker are not required.
3. With the CLI, supply the code on stdin to
   `infuser connect --server HTTPS_ORIGIN --app APP`. It writes
   `.local/APP.credentials.json`. Keep `.local/` and `*.credentials.json` out of
   Git and game exports. Never print secrets or put them in command arguments.
   Reuse this file for updates. If the code expires, use **Refresh code** for the
   same app; do not reserve another game.
4. Read `GET /api/publisher/capabilities` with the connection's bearer token.
   Integrate only features this installation advertises. Never give a game its
   publishing credential or the operator's control token.

## Choose the integration

| Existing project | Instructions |
| --- | --- |
| Three.js, Vite, or plain HTML/JavaScript | [Browser games](engines/threejs-vite.md) |
| Browser game using PeerJS | [PeerJS adapter](peerjs.md), including standalone play |
| Browser game with a Node HTTP/WebSocket server | [Managed Node hosting](engines/node-server.md): upload source; Infuser packages and runs it |
| Godot web | [Web export and saves](engines/godot-web.md), then [multiplayer](engines/godot-multiplayer.md) if needed |
| Godot native, single-player Windows/Linux | [Infuser Play preview](engines/godot-native.md) |

Godot and PeerJS adapters are previews with the limitations in their recipes.
Managed Node hosting is implemented and tested. Check this installation's
`server_packaging` capability; an old proposal or missing dedicated Join Friend
support is not a reason to reject Node/WebSocket hosting.
[Bevy](engines/bevy.md) has no validated integration yet. Do not promise support
based only on an engine's ability to export a web page.

For new integrations, declare [SDK/API major 1 and required capabilities](sdk-versioning.md)
in `infuser.game.json`; import `/_infuser/sdk/v1.js`. Major 1 is still a preview,
not a frozen SDK 1.0. Existing games keep their declared version unless deliberately
migrated. [Achievements are not implemented](sdk-contract.md).

## Explicit play modes

Declare the game's actual modes in `infuser.game.json`. `play_modes` accepts
`["solo"]`, `["multiplayer"]`, or both; `solo` is the API spelling of Single player.
For single player, set `solo_runtime` to `client_only` or `shared_server`.
Multiplayer also declares its topology, transport, capacity, and join policy.
Copy the complete example from the matching multiplayer recipe.

- **Peer-hosted:** one player hosts the shared room. WebRTC requires Infuser TURN
  fallback, even when LAN or direct internet connections work. Use the
  [browser peer SDK](peer-hosting.md), Godot adapter, or PeerJS adapter as appropriate.
  Test with relay forced; LAN-only and direct-only networking are unsupported.
- **Dedicated / persistent world:** one managed server per game. Plain Node HTTP/WebSocket
  games can use the [Node server recipe](engines/node-server.md) when advertised by
  the installation. Other runtimes and persistent worlds use the advanced staged-image
  [publishing API](public-api.md). Use [on-demand sessions](runtime-sessions.md)
  when the title screen or single-player mode does not need the backend.
- The separate `relayed` datagram/WebSocket mode is **not implemented**.

Engine choice does not determine networking. Never relabel multiplayer as single
player to get an upload accepted, or create another room after a failed join.
Play opens the game's own title screen. [Presence](presence.md) and optional
[Join friend](launch-intents.md) let that screen show and join existing players.

## Saves and presentation

Wire saves into real load/checkpoint operations. Use conditional writes and show
conflicts without discarding either version. For related JSON or binary files, use
[declared checkpoints](cloud-save-design.md). Guest launches must start empty and
never import or overwrite the owner's progress. Infuser supplies the isolated
identity and storage; the game must use them.

Choose up to eight initial tags from the prompt or `GET /api/publisher/tags` and
put them in `infuser.game.json`. Later publications preserve author tag edits.

Choose **one** Library art handoff:

- Upload a cover meeting the [art spec](card-art-guide.md), at
  `infuser-art/card-hero.webp`. Suitable existing art or a HUD-free screenshot is fine.
- If you cannot produce high-quality artwork, save an `art_prompt` through the
  [metadata API](game-metadata.md) after publication. Describe the real game,
  recognizable subjects, visual style, mood, and anything to avoid. An Infuser art
  agent can read this request and upload the cover later. Do not upload inferior
  placeholder art. Tell the child the game is playable and artwork is requested.

The fallback cover remains until artwork arrives. A prompt is a valid handoff,
not a failed publication; do not claim an image is ready or promise a completion time.

After first publication, use the [metadata API](game-metadata.md) to save a short,
factual `description`: what players do, the game's setting, and any distinctive
visuals. Read existing metadata before editing; preserve an author's description
unless the game has changed. This gives future agents context for artwork and
updates. Cover, description, and Library-title edits do not require a new build.

## Publish and verify

For a server-backed game, use the current [publisher download](publisher-install.md).
On failure, read its build log or [runtime diagnostics](server-diagnostics.md)
before asking the child for help. Reuse the existing app and credentials.

Use the project's existing build tools, then publish from its root with the
built output path. For example, after building a Vite game:

```sh
infuser publish .local/APP.credentials.json . --web-dir dist
```

This reads `infuser.game.json` and project artwork. `--web-dir` skips building;
it must point to an existing export. Follow the engine recipe for other formats.
Upload only public runtime assets. The CLI injects the SDK into uploaded HTML;
it does not implement saves or networking for you.

Verify a real launch, save/reload, guest isolation, and every declared multiplayer
mode. Check the Library title, tags, description, and cover. Only adult admins can
play `--preview` publications; do not hide a child's game from them by using it.
An upload alone is not completion. A multiplayer update may be **queued** until
its active session ends; check `publication_state` and report that accurately.

“Update it on Infuser” means build and publish with the same connection and app ID.
“Publish every update” is an instruction to the developing agent, not an Infuser
watcher. Games do not need a GitHub repository. Failed uploads leave the active
build intact. `infuser unpublish CREDENTIALS` hides a game and expires sessions,
but retains builds and personal saves; use it only when requested or cleaning up
an agreed disposable test.

## Reporting platform problems

For problems with Infuser, Infuser Play, publishing, or the SDK:

1. Reproduce the failure and check the recipe and server capabilities. Fix ordinary
   game-code mistakes locally. Report unsupported features as requests, not bugs.
2. Search [open and closed issues](https://github.com/esper256/infuser/issues).
   Add evidence to a matching open issue; link a closed issue if it has regressed.
3. Use your connected GitHub tool or authenticated CLI to file a short report:
   steps, expected/actual results, relevant versions, a sanitized error excerpt,
   impact, and any workaround. Give the child the confirmed issue link.

Reports are public. Exclude the copied prompt, codes, credentials, cookies, launch
URLs, TURN secrets, private domains/IPs, account details, and private game data.
Use example domains and paths. Review screenshots and logs before including them.
Bring security-sensitive findings to the adult admin privately.

If GitHub access is unavailable, save the sanitized report in
`.local/infuser-issue.md` for an adult and say it has **not** been filed. Do not ask
the child for GitHub credentials. Continue safe workarounds without wiping saves,
weakening authentication, or quietly dropping multiplayer.

## Private groups

The publishing connection assigns the game to its creator’s private group. Do
not ask the child for a group ID or add one to the manifest. Games, players,
presence, and saves are scoped by Infuser; the SDK integration stays the same.
See [private groups](private-groups.md) for operator setup.
