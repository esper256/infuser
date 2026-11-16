# CI and dependency maintenance

Audience: Infuser developers.

PR checks are advisory: fast Rust journeys (`cargo test --locked`), Node 24 SDK
and theme tests (`node --test tests/*_sdk.mjs tests/themes.mjs`), Rust formatting
(`cargo fmt --all -- --check`), and Clippy
(`cargo clippy --locked --all-targets -- -D warnings`). Each check runs independently
with `continue-on-error`; inspect job/step results even if the overall check is green.
There is no approval environment, required check, or release dependency added here.
The release workflow remains separately maintained.

The **Docker and Caddy integration (manual)** workflow runs only via Actions →
Run workflow. It builds the demo
image, runs the ignored Docker lifecycle journey, and runs the ignored Caddy web
journey with the tracked production Caddy build. It also exercises managed Node
packaging and backup/restore. It does not deploy or publish an image. See
[local integration commands](developer-testing.md#docker-and-caddy) for the same test entry points.

Dependabot checks the server and desktop Cargo manifests, PeerJS npm packages,
GitHub Actions, Docker/Compose definitions and Caddy Go dependencies weekly.
Minor/patch version updates are grouped within each configured entry; major
updates stay separate. Open version-update PR limits are three or five per
entry, as specified in [the configuration](../.github/dependabot.yml).
Security updates have separate groups where configured and depend on repository
security-update settings. Adding a manifest does not change the release queues
or make Mac builds a prerequisite for server releases.
See [GitHub's configuration reference](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference).

The standalone **Dependency audit (advisory)** workflow runs weekly and manually,
using cargo-audit 0.23.0 and the current RustSec database. Findings and operational
errors from both the server and desktop lockfiles appear in the Actions job summary and warning annotations. It neither opens
individual issues nor blocks releases. This checks both Cargo.lock files, not container OS
packages or the code of Actions.

Native Dependabot alerts, the dependency graph and automated security-update PRs
are repository settings. This change does not enable or alter them; security groups
apply if those features are enabled. A repository administrator can review those
settings separately. See [GitHub's security-update guidance](https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/secure-your-dependencies/configure-security-updates).

The advisory workflows pin Actions to full commit SHAs with readable version
comments. The release workflows retain their own action references; see those
files before changing them. The installed Rust toolchain follows stable.

## Release builds

`Build and release` tests and builds both Linux server architectures and both
Infuser Play platforms. It publishes a complete `build-<commit>` release with
same-commit binaries, installers, manifests and checksums. Existing updater URLs
and installer aliases are retained. No Mac job is on this critical path.

A running build finishes while new pushes replace only the pending run, so a
burst consumes one running build plus the newest pending candidate rather than
continually cancelling useful work. A tested ancestor of main may publish even
if main advances. Publication checks ancestry and the current latest release;
an older rerun cannot replace a newer release. Missing assets, failed tests,
unknown ancestry or upload failure leave the release unpublished. Published
assets are never replaced. A queued candidate can still fail and require a retry.

`Mac publisher release` follows successful main server workflows and builds the
exact upstream commit. Its independent queue publishes the two tested Mac
architectures as `macos-<commit>` with its own manifest and checksums. These are
marked pre-release to prevent them becoming the server/Play `latest` release;
this is channel separation, not a waiver of tests. Mac capacity delays affect
only Mac publisher availability. Use the releases list, not `latest/download`,
for Mac downloads. Manual dispatch on main can retry that channel independently.

Both tracks use standard GitHub-hosted runners and no paid service. Linux
server publication still waits for Windows/Linux Play packaging to preserve
compatibility with existing clients that share GitHub's single latest channel.
Publication does not prove host installation: the updater may defer for active
games or roll back a failed activation. Verify the host build separately.

Server operators enable [automatic updates](deployment.md) separately.
