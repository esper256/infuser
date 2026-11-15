# Put an existing game on Infuser

Start with the game's engine. Do not ask the child to configure a server, Docker,
DNS, or ports. Keep their existing game and build workflow intact.

The game-making agent owns the build and publishing loop. “Update it on Infuser”
means build using the project's existing tools, then invoke `infuser publish`.
“Publish every update” is an instruction to that agent, not an Infuser watcher.
Supply prebuilt output with `--web-dir dist` (or the project's actual export path);
automatic build detection is only a convenience. GitHub is not required for games.

Three.js/Vite is the first supported journey. Godot web and native Godot are
preview integration studies; use the tested recipes below and report unsupported
features explicitly. Bevy remains paused while these contracts settle.

1. Inspect this project for an existing integration with the target Infuser.
   Reuse its game-scoped credentials and immutable app ID if present, even if the
   new prompt offers a different reserved ID; leave that unused setup code alone.
   Do not duplicate an already-connected game. Otherwise the copied prompt
   contains a one-time setup code, server origin, and app ID. Supply the code on
   stdin to `infuser connect --server HTTPS_ORIGIN --app APP`. The CLI exchanges it
   once and stores `.local/APP.credentials.json`. Do not echo secrets into logs or
   shell history. Ignore `.local/` and `*.credentials.json` in Git and exports.
   The code expires in one hour; the child can use **Refresh code** for the same
   app. An operator-supplied credentials file also works. Never create another
   app just to refresh credentials.
2. Inspect the project, then choose the closest recipe:
   - **[Three.js / Vite / browser JavaScript](engines/threejs-vite.md)** — first supported journey.
   - **[Godot web export](engines/godot-web.md)** — export and adapter path.
   - **[Godot multiplayer](engines/godot-multiplayer.md)** — explicit transport choices; recipe in progress.
   - **[Godot desktop](engines/godot-native.md)** — Infuser Play for Windows/Linux and single-player exports.
   - **[Bevy](engines/bevy.md)** — browser export first; recipe in progress.
3. Declare the [SDK/API major and required capabilities](sdk-versioning.md) for
   new integrations; use the matching versioned runtime import. Query
   `GET /api/publisher/capabilities` with the pass's bearer token before
   choosing a multiplayer integration. Engine choice does **not** tell you the
   game's topology. Solo and multiplayer may both be supported.
4. Use the Infuser CLI matching this server's current release. Linux AMD64/ARM64
   executables are attached to this repository's releases; verify their SHA-256
   against `release.json`. Publishing currently needs a Linux workspace: native
   Windows/macOS agents should use WSL or a Linux cloud/development environment.
   Infuser Play installers are player applications, not a Windows publishing CLI.
5. Integrate saves, publish, then exercise a real player journey. An upload alone
   is not completion. Use `--preview` for admin-only dogfood/test games.
6. If the child reports a platform problem or you hit one while integrating,
   follow [Reporting platform problems](#reporting-platform-problems) below.
   Handle the investigation and GitHub report; the child should not need to
   diagnose the SDK or manage a GitHub account.

The child opens **Add a game**, copies the prompt, and pastes it into the chat
where they built their game. There is no name form or file management. Infer the
existing title from the project and put it in `infuser.game.json`; do not rename
the game or start a new project. First publication assigns its readable URL.
The dialog shows the API-provided supported engine list; do not promise an
unvalidated adapter for an engine outside that list.
Only adult admins can invite or disable members. A disabled member's
publisher credentials and player sessions stop working.

**SDK status: pre-1.0.** Read the [compatibility review](sdk-contract.md). The current
integration is for dogfooding; do not mistake manifest schema 1 for SDK 1.0 or
invent APIs for achievements. Play opens the
game's own title screen; keep its Single player/Multiplayer selection intact.

Infer a few initial tags from the actual game. Prefer the supplied catalog or
`GET /api/publisher/tags`; put up to eight tag IDs in `infuser.game.json`, for
example `"tags":["3d","arcade","chill"]`. Later publishes preserve author edits.

For Library card artwork, follow the draft [card hero art guide](card-art-guide.md).
Keep `infuser-art/card-hero.webp` in the project; `infuser publish` validates and
attaches it automatically when the server advertises `card_art`. Review the art
in the Library after publication. No artwork manifest field or game SDK call is
needed; source masters and notes stay in the project. For a cover-only or Library
title-only change to an existing game, use the [metadata API](game-metadata.md)
with its existing connection file. Do not rebuild or republish the game just to
replace its cover. Explicit artwork overrides take precedence over build art
until cleared; keep the source copy in the project as well.

## What Infuser handles

- HTTPS, game hosting at `/` on an isolated hostname, MIME types, range requests,
  health-aware server startup, and idle shutdown.
- A small browser SDK, automatically included in the uploaded entry page by
  `infuser publish`, maintains the session while the game is open.
- Cloud-save namespaces bound to the signed-in player and game; guest play gets a
  new empty namespace **and a new browser origin**. Never copy local progress
  into guest saves or fall back to the owner's identity.
- [Declared JSON checkpoints](cloud-save-design.md), when capabilities advertise
  `cloud_checkpoints.version = 1`. Submit complete groups, handle preserved
  conflicts explicitly, and reuse the same commit ID when retrying a write.
- Immutable builds. Existing browser sessions keep their selected build. Server
  updates are queued while the shared server is active and activate automatically
  after it sleeps. Check `publication_state` on the build to distinguish an upload
  from an activated update; the CLI reports this distinction.
- One shared server per game. A second player joins it; never create a parallel
  lobby/world merely because a connection fails.
- [On-demand managed server sessions](runtime-sessions.md): opt in with
  `"server_start":"on_demand"`, then acquire when a mode needs the backend and
  release when it does not. Title screens and client-only single player use no
  server reservation. Peer-hosted games use `peers.join()` instead.
- [In-game presence and peer joinability](presence.md): query `presence.get()`
  on the title screen, distinguish online/game open/multiplayer, and join the
  observed session ID. Only the host advertises when new players may join.
- [Join friend launch intents](launch-intents.md): opt in explicitly, consume
  once, join the supplied friend/session, and fall back to the title screen if
  unavailable. Ordinary Play retains the game's normal mode selection.

## Explicit play modes

For an existing solo game, `--solo` is the only gameplay declaration needed.
For dual-mode or multiplayer games, supply `infuser.game.json`, for example:

```json
{
  "title": "Moon Racers",
  "play_modes": ["solo", "multiplayer"],
  "solo_runtime": "client_only",
  "multiplayer": {
    "mode": "dedicated",
    "transport": "websocket",
    "max_players": 8,
    "join_policy": "anytime"
  }
}
```

A server recipe adds its managed server settings. Do not invent support for an
unavailable runtime. `peer_hosted` always requires relay fallback, even if direct
connection is an optional optimization. Experimental [WebRTC peer hosting](peer-hosting.md)
is available on installations configured with TURN; check capabilities first.
The separate datagram/WebSocket relay mode is not implemented. There is no
LAN-only/direct-only fallback.

A [readable game link](game-urls.md) is assigned automatically. Keep the immutable
app ID for updates. A slug rename preserves old entry links and cloud saves.

## Updates and cleanup

Repeat the same publish command using the same game pass. The CLI creates a new
immutable build and promotes it. A failed upload leaves the working game alone.
An abandoned build can be inspected/deleted through the publishing API.

```sh
infuser publish /private/game.credentials.json /path/to/project --solo
infuser publish /private/game.credentials.json /path/to/project --solo --preview
infuser unpublish /private/game.credentials.json
```

Unpublish removes the game from the library and expires its sessions, preserving
builds and personal saves. Unused builds can then be deleted to reclaim disk.

The low-level [public API](public-api.md) is the escape hatch for custom pipelines,
not the starting point. Never give a game the operator control token or its
publishing credential. Keep individual game secrets out of container images.

## Reporting platform problems

Report problems with **Infuser** (the website), **Infuser Play** (the native app),
publishing, the SDK, saves or multiplayer infrastructure in
[esper256/infuser issues](https://github.com/esper256/infuser/issues). This applies
both when a child says something is broken and when the developing agent hits a
platform error, unclear instructions or a missing capability.

1. Reproduce the problem and check the current recipe/capabilities. Fix ordinary
   game-code mistakes in the game project. If the evidence points to Infuser, or
   documented behavior still fails and the cause is uncertain, report what you
   observed. Describe an unsupported capability as a request, not a proven bug.
2. Search existing open **and closed** issues for the same symptom. Add useful
   new evidence to a matching open issue instead of filing a duplicate. If a
   closed issue has regressed, open a new report and link it.
3. File a concise issue using the agent's connected GitHub tool or authenticated
   GitHub CLI. Include the affected component, reproduction steps, expected and
   actual behavior, relevant versions, and a small sanitized error/log excerpt.
   Note how often it happens, whether it blocks play or publishing, and any
   workaround tried. A child's description is useful context; translate it into
   reproducible steps without requiring the child to supply technical details.
4. Give the child the issue link and a short explanation of what still works.
   Continue with a safe workaround when possible. Keep their game, connection,
   builds and saves intact; never hide a platform failure by wiping progress,
   weakening authentication or quietly dropping multiplayer.

Reports are **public**. Do not paste the Add Game prompt, setup codes, credential
files, cookies, authorization headers, launch URLs/tickets or TURN credentials.
Replace private server domains/IPs, account details and local paths with examples
such as `games.example.com` and `/path/to/project`. Share only a minimal sanitized
reproduction, not the child's private source tree, saves or an unreviewed log
bundle. Review screenshots too. If a suspected security vulnerability or sensitive
report cannot be safely described publicly, bring it to the adult admin privately.

For the CLI, write the reviewed report to an ignored local file, then use:

```sh
gh issue list --repo esper256/infuser --state all --search "brief symptom"
gh issue create --repo esper256/infuser --title "[SDK] Brief concrete symptom" \
  --body-file .local/infuser-issue.md
```

Use the GitHub account already available to the agent. Infuser publishing tokens
are **not** GitHub credentials. If GitHub access is unavailable, keep the sanitized
report in `.local/infuser-issue.md` for an adult to submit, provide the issue-page
link, and clearly say it has **not** been filed. Do not ask the child to create an
account, paste a token, or do the debugging. Never claim an issue was filed without
a confirmed issue URL.
