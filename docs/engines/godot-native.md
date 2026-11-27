# Native Godot preview

Audience: game authors and their agents.

**Creating a game?** Read this recipe for engine constraints, then return to the
[creation guide](../create-game.md). Skip connection and publishing commands for now.
**Integrating an existing game?** Start with the [publishing checklist](../agents.md),
connect once, then apply this recipe. Return to
[Explicit play modes](../agents.md#explicit-play-modes) and finish that checklist's
saves, artwork, publishing, and handoff steps.

Windows x86-64 and Linux x86-64 (including Bazzite) use **Infuser Play**, the native app for the Infuser website.
The user installs **Infuser-Play-Setup.exe** or opens **Infuser-Play.flatpak** in their
software center, then presses Play in the website. No manual unzip, copying game
files, terminal commands or per-game installation. The first launch opens the
browser to connect their Google account. Later launches download missing updates
and start the game. Guest play uses a fresh local directory and cloud identity.

This preview supports **client-only single-player Godot/GDScript exports**.
Native multiplayer, ARM desktop clients and macOS are not included. Game updates
and [Infuser Play updates](../play-updates.md) are automatic. Installations from before
companion updates were added need the current installer once. Account data and saves
are retained.
The unsigned Windows preview may show a Windows security prompt. The complete
login-and-play flow has not yet been verified on a physical Windows device.

## Adapt an existing project

Keep its current title screen, project name and gameplay. Copy
`examples/godot-web/addons/infuser` and register `Infuser` as the first autoload.
Put the play modes, SDK requirement, and cloud groups in `infuser.game.json`
as in the [existing-project example](../../examples/godot-existing/infuser.game.json).
Use the same checkpoint calls as the [web recipe](godot-web.md), and the
same reusable conflict chooser. `ready` provides the player and guest identity.
Native calls use HTTPS and a scoped game credential, not a browser bridge.

Keep Godot project-setting overrides enabled. Infuser Play writes `override.cfg`
beside the launched executable **before any autoload or user:// access**, selecting
a per-server/player/game directory. Guest launches get unique directories, removed
after the game exits. The adapter refuses API access if the actual user directory
does not match. Do not redirect saves to an absolute home path, read another
game's directory, or ignore failed save loads. This is save separation, not an OS
sandbox against a malicious native game.

## Publish Web and Infuser Play together

Use the existing game's credentials and project root for both formats. In
`infuser.game.json`, declare `"play_modes": ["solo"]` and
`"solo_runtime": "client_only"`. Use the same save declarations in both exports.

Build release presets from the same revision into **separate directories**.
If you export inside the project as shown below, put an empty `exports/.gdignore`
in the parent directory so Godot skips previous exports when importing resources.
Use the [Web export recipe](godot-web.md) for `exports/web/index.html` and its
sibling files. Each desktop directory needs one root executable plus its PCK
and resources. For Windows, disable the console wrapper in the release preset
so the directory contains just one `.exe`. Include only the platforms you built. For example, from the
directory containing `project.godot`:

```sh
infuser publish .local/GAME.credentials.json . --web-dir exports/web \
  --native-linux exports/linux --native-windows exports/windows
```

Omit a native flag if you only built the other platform. Omit `--web-dir` if
you want the publisher to build the project's Web preset. The CLI packages the
desktop exports, generates their file inventories and hashes, and publishes all
formats as one build under one Library card. Each native ZIP supports up to
64 MiB compressed, 512 MiB expanded, and 512 files.

On a computer with a matching desktop export, open the expanded game card and
try **Play online** and **Launch in Infuser Play**. The compact card's **Play**
opens the browser version directly, and its dropdown also offers both choices.
On other platforms, players can still play the Web export.

For each update, repeat the combined command with fresh exports. Each published
build contains exactly the formats you include in that publish. To deliberately
publish a desktop-only build, pass `--native-only` and the native export flags.

## Handoff and update contract

Desktop delivery is a separate **preview v1** protocol, not SDK 1.0 stability.
Google-authenticated Play creates a ten-minute, one-use request. Device pairing
uses a verifier held by the app and an explicit signed-in browser approval; the
device credential belongs to that account. Another player's launch cannot be
claimed with it. Revoking a device or disabling membership prevents new claims.
The long-lived credential stays in the companion's private application data;
games receive only their scoped, heartbeat-renewed launch token.

The companion verifies archive and per-file hashes, rejects path traversal,
symlinks and undeclared files, and atomically installs a complete version. Each
run gets its own copy, so a new update cannot replace a running game's files.
Corruption leaves the old cached version intact and reports an error. A failed
update never silently substitutes a different version or player identity.
