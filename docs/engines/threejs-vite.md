# Three.js / Vite: first-class browser publishing

Audience: game authors and their agents.

Use this recipe for existing Three.js/Vite or plain HTML/JavaScript games. Other
browser engines may produce compatible exports, but their adapters have not been
validated by this recipe.

**Creating a game?** Read this recipe for engine constraints, then return to the
[creation guide](../create-game.md). Skip connection and publishing commands for now.
**Integrating an existing game?** Start with the [publishing checklist](../agents.md),
connect once, then apply this recipe. Return to
[Explicit play modes](../agents.md#explicit-play-modes) and finish that checklist's
saves, artwork, publishing, and handoff steps.

## First publish

For new integrations, declare `"sdk":{"major":1,"api_major":1,
"required_capabilities":["identity.v1","saves.v1"]}` in `infuser.game.json`.
Read the [versioning contract](../sdk-versioning.md); this major is still preview.
Existing integrations may retain their legacy SDK until deliberately migrated.

Inspect `package.json`. Use the project's existing package manager to install
its locked dependencies. For a normal Vite project:

```sh
npm ci
infuser publish /private/moon.credentials.json . --solo --title "Moon Racers"
```

Infuser detects Vite, runs `npm run build`, uploads only `dist`, inserts the SDK
script in the uploaded `index.html`, and publishes. It does not change source
files, rewrite asset paths, or ask for a container. A game is served at its own
origin's `/`, so existing root-relative paths work.

For a different output directory, use `--web-dir build`. This means **already
built**: run the project's build first. A plain HTML/JS game can publish its
export folder directly with `--solo`. Do not publish a source tree containing
private files. The CLI rejects symlinks and credential files; its export directory
should still contain only public game assets.

## Add cloud saves to the actual gameplay loop

The CLI includes the SDK for session maintenance. Integrate the game's real save
and load operations as well; automatically including a script cannot know what
progress means in your game. Load the SDK at runtime so Vite does not try to
bundle the server-provided file:

```js
const sdkUrl = new URL('/_infuser/sdk/v1.js', location.origin).href;
const infuser = await import(/* @vite-ignore */ sdkUrl);
const session = await infuser.ready;
let save = await infuser.saves.load('progress');
const progress = save.data ?? { level: 1, stars: 0 };

// At a checkpoint, after changing progress:
save = { ...save, ...(await infuser.saves.save('progress', progress, save.version)) };
```

`session.guest` and `session.name` can label a guest run. The SDK handles which
save namespace to use. Keep its version value; a `409` means another device
changed the save. Reload and resolve the game's data deliberately, rather than
retrying with a guessed version. Include your own save schema version in `data`.
Handle network/session errors visibly; do not say “saved” until the write succeeds.
For standalone play, use [optional platform detection](../peerjs.md#keep-one-project-working-in-both-places)
to select the existing local save/networking path when Infuser is absent. A failed
hosted cloud write must never silently switch to local saves.

## Verify the experience

First follow [publication verification and handoff](../agents.md#publish-and-verify)
using the publisher API. The live checks below apply when an authorized game test
session is already available. Otherwise, report them as untested and hand off the
published game; do not ask for Google login or account access to complete them.

1. Publish normally for the owner to test. Use `--preview` only for admin tests.
2. Play from the Library, earn progress, close and reopen, and confirm it loads.
3. Choose **Play in guest mode**: progress must start empty. Earn different progress.
4. Reopen ordinary play: the owner's original progress must be unchanged.
5. Publish a visible change. Existing tabs retain the previous build; a fresh Play
   receives the new one. Inspect missing assets rather than hiding them with SPA fallback.
6. Remove agreed disposable test games when finished; keep the actual game published.

For multiplayer, declare the real mode and transport in `infuser.game.json` and
check capabilities first. Do not label an online game solo to get it published.
Use [WebRTC peers](../peer-hosting.md) or the [PeerJS adapter](../peerjs.md) for
compatible peer-hosted games. For a Node/WebSocket backend, use the
[managed Node recipe](node-server.md) when the installation advertises it.
Other server runtimes still use [staged images](../public-api.md).
