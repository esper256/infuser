# Three.js / Vite: first-class browser publishing

**Goal:** an agent with an existing game and a game pass should need no hosting
configuration. This also covers ordinary Vite/React, Phaser, Babylon.js, and
vanilla browser exports, though Three.js is the first dogfood target.

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
Outside Infuser, a development-only local save adapter is fine, but do not
silently fall back to local saves when a production cloud write fails.

## Verify the experience

1. Publish with `--preview` during development (adult admins only).
2. Play from the Library, earn progress, close and reopen, and confirm it loads.
3. Choose **Play in guest mode**: progress must start empty. Earn different progress.
4. Reopen ordinary play: the owner's original progress must be unchanged.
5. Publish a visible change. Existing tabs retain the previous build; a fresh Play
   receives the new one. Inspect missing assets rather than hiding them with SPA fallback.
6. Unpublish the test game before making the final game visible if it is disposable.

For multiplayer, declare the real mode and transport in `infuser.game.json` and
check capabilities first. Do not label an online game solo to get it published.
The managed WebSocket server recipe is next; custom advanced server settings
exist but should not be the first question the agent asks the child.
