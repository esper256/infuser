# Native Godot preview

Windows x86-64 and Linux x86-64 (including Bazzite) use **Infuser Play**, the native app for the Infuser website.
The child installs **Infuser-Play-Setup.exe** or opens **Infuser-Play.flatpak** in their
software center, then presses Play in the website. No manual unzip, copying game
files, terminal commands or per-game installation. The first launch opens the
browser to connect their Google account. Later launches download missing updates
and start the game. Guest play uses a fresh local directory and cloud identity.

This preview supports **client-only single-player Godot/GDScript exports**.
Native multiplayer, ARM desktop clients and macOS are not included. Game updates
and [Infuser Play updates](../play-updates.md) are automatic. Installations from before
companion updates were added need the current installer once. Account data and saves
are retained.
The unsigned Windows preview may show a Windows security prompt. An actual Windows
user must complete the acceptance checklist before broad distribution.

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

Export standard release presets into separate directories, each containing one
root executable plus its PCK/resources. Use the Compatibility renderer as the
starting point for laptop compatibility. Keep secrets and development files out.
Then the developing agent publishes:

```sh
infuser publish .local/GAME.credentials.json . --native-only \
  --native-linux exports/linux --native-windows exports/windows
```

Omit `--native-only` to upload Web and native formats in the same immutable build.
Web remains the main Play action; **Open in Infuser Play** appears in its dropdown.
Native-only builds use the main Play action directly. Each native ZIP is limited
to 64 MiB compressed / 512 MiB expanded; up to 512 portable file paths. The CLI
creates the archives, inventory and SHA-256 hashes. Agents do not author Docker
files or native-delivery manifests by hand.

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

## Windows handoff checklist

1. Open the normal installer under a non-admin Windows account. No unzip step.
2. Press Play on a native preview in the Library; allow the Infuser Play link handler.
3. Approve the browser login handoff; confirm the game opens under the right player.
4. Save, close and reopen. Then use guest mode: blank progress, owner preserved.
5. Publish an update. Next Play downloads it; saves remain. A running older game
   stays intact. Try a disconnected launch and confirm the error is understandable.
6. Close the game and confirm Infuser Play exits. Uninstall/reinstall the companion and
   confirm personal saves remain. Check a laptop with scaling enabled too.

Actual Windows testing is pending; Linux results do not substitute for it.

The opt-in `tests/godot_native_journey.mjs` uses a loopback TLS fixture and an
export of the adapted Dodge project. On the Bazzite laptop it passed verified
download, real native Godot startup, cloud checkpoint/reload, persistent owner
user://, blank guest user://, guest cleanup and unchanged owner progress. Its test
certificate and acceptance autoload belong only in that disposable project.
`desktop/examples/acceptance.rs` supplies that CA to a test client; the installed
companion has no test-certificate override. Archive corruption/update isolation
and device-pairing/claim/revocation are also covered by fast Rust journeys.
The CI-built Flatpak was installed and started on Bazzite with the standard
Freedesktop 25.08 runtime, and its `infuser://` handler was registered. The Windows
installer compiled successfully and desktop unit journeys passed on the Windows
CI runner. Google login plus a full installed-app play session still needs a
human account acceptance pass; mock login tests do not certify that interaction.
