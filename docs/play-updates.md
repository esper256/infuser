# Infuser Play updates

Install Infuser Play once. When opened, it checks for a passing GitHub release at
most once an hour, then downloads and starts the updated companion automatically.
The window shows progress; no second installer or administrator password is needed.
An update check needs internet, but a failed check leaves the installed copy usable.
Game updates remain a separate step after the companion starts.

The installed executable is a stable launcher. Windows protocol registration and
shortcuts keep pointing to it; the Flatpak executable remains in its read-only
installation. Updated executables live in `play-updates` inside Infuser Play's
application data directory. Updating does not change account connections, personal
saves, guest namespaces, or running games. An already running companion finishes
its session with its original version.

Only this project's published GitHub releases supply updates. Game manifests and
Infuser servers cannot choose the companion executable. `play-release.json` names
a full commit and SHA-256 digest for each platform. Downloads use that commit's
release URL, have bounded size/time, and must match the digest before an atomic
install. HTTPS protects the manifest; checksums are not independent code signatures.
The Windows preview remains unsigned.

A new worker must create its window and acknowledge readiness before the launcher
closes its window. The launcher then waits without a window until the worker exits,
keeping Flatpak’s sandbox alive. A failed startup falls back to the previous verified worker, then the
bundled copy. The failed release is skipped until another release appears.
The active and previous workers are retained. A per-user lock prevents simultaneous
launches from racing update activation. No update interrupts an existing game.
This handshake checks startup, not every game operation; release tests still matter.

Development builds without `INFUSER_BUILD_SHA` do not download updates. Forks should
change the release repository in `desktop/src/update.rs` when packaging their own
companion. Existing preview installations from before this feature need the new
installer once; later releases update themselves.
