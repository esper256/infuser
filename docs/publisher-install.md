# Publishing from a Mac or Linux

Audience: game authors and their agents.

These instructions are for the game-making agent. The user copies the **Add a
game** prompt into their existing project chat; the agent handles the rest.
Keep the project's current engine and build tools. Installing Rust, Docker, or a
Linux VM is not required to publish from a Mac.

Download the publisher from [Infuser releases](https://github.com/esper256/infuser/releases).
Prefer the server's release; if it predates Mac downloads, use a newer publisher
and check `/api/publisher/capabilities` before integrating features. The publisher
uses the same HTTP API and credentials on every supported publishing platform.

| Agent machine | Release asset |
| --- | --- |
| macOS, Apple Silicon (`uname -m`: `arm64`) | `infuser-macos-arm64` |
| macOS, Intel (`uname -m`: `x86_64`) | `infuser-macos-amd64` |
| Linux, ARM64 (`aarch64`) | `infuser-linux-arm64` |
| Linux, x86-64 (`x86_64`) | `infuser-linux-amd64` |

Select one immutable release tag, then download that asset and its `release.json`
or `SHA256SUMS` from the **same tag**. Verify the asset's SHA-256 before running it
(`shasum -a 256 FILE` is available on macOS; Linux has `sha256sum FILE`). A missing
asset or mismatched checksum is a stop, not a reason to skip verification.
Store the verified executable as `.local/bin/infuser`, run `chmod 700` on it, and
invoke that path in the engine recipes. Ignore `.local/` in Git and exports.
It need not be installed globally or run with administrator privileges.

The Mac binary exposes `connect`, `publish`, `unpublish`, and `version`; it does
not run the Linux hosting daemon. macOS 15 is the validated baseline; older macOS versions have not been validated. These downloads are not Apple Developer ID signed or notarized;
if local execution needs admin approval, use the HTTP route below while that is
resolved. Do not disable OS protections or install a compiler to work around it.

Pass the one-time setup code through stdin to `connect`, never a command-line
argument. It creates a private credentials file without overwriting an existing
one. Reuse that file for every update to the same game. Public HTTPS works without
extra options; privately operated certificate authorities can use `--ca-cert FILE`
with their trusted PEM certificate. Certificate verification remains enabled.

If downloading/running a binary is unavailable, [publish with HTTP](publishing-http.md)
using tools already present. This also serves native Windows agents without WSL.
Respect the development agent's execution permissions; the alternative does not
override its approval requirements.

**Publishing and playing are different.** Mac players can play web games in their
browser. Infuser Play currently supports native games on Windows and Linux
(including Bazzite); this Mac publisher does not add native Mac game delivery.
