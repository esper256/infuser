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
GitHub Actions, Docker/Compose definitions, Caddy Go dependencies and Ansible
operator requirements weekly. Ansible series upgrades require review for host
Python compatibility; patch updates remain automated proposals.
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

Each server release also gets a changelog of the merged pull requests and
commits since the previous `build-<sha>`. The text is the GitHub release
notes and a `changelog.json` asset. It is compiled into the server binary
rather than added to `release.json`, because an updater that is already
installed rejects unknown manifest fields. Pull request titles are the
lines families may read. The path rules and the optional `family:` /
`internal:` title prefixes are in the repository
[agent instructions](../AGENTS.md). Internal-only releases stay in the notes
and off the Recently feed.

A running build finishes while new pushes replace only the pending run, so a
burst consumes one running build plus the newest pending candidate rather than
continually cancelling useful work. A tested ancestor of main may publish even
if main advances. Publication checks ancestry and the current latest release;
an older rerun cannot replace a newer release. Missing assets, failed tests,
unknown ancestry or upload failure leave the release unpublished. Published
assets are never replaced. A queued candidate can still fail and require a retry.

`Mac publisher release` checks daily at **06:23 UTC**, using a cheap Linux job.
It selects the latest published `build-<commit>` server release and verifies that
commit belongs to main. It builds only when Mac inputs differ from the nearest
completed Mac release in that commit's ancestry (or no such release exists).
The conservative input set includes the shared Rust source, Cargo dependencies,
native protocol, SDK/examples, publisher journey fixtures and release tooling.
Docs, web UI and desktop-only changes do not trigger a Mac pair. Failed or draft
Mac releases are not a baseline, so the next daily check retries them. GitHub
may delay scheduled jobs; this is a daily batch, not an exact delivery deadline.

For an urgent publisher fix or retry, open **Actions → Mac publisher release →
Run workflow**, select **main**, and run it after the server release publishes.
Manual dispatch builds the latest published server commit even if the input
filter finds no change. Both triggers skip an already complete exact Mac
release. Dispatches on other branches are skipped. The independent queue never
cancels a running pair; bursts can replace the pending run.

Both tested Mac architectures publish together as `macos-<actual-commit>` with
their own same-commit manifest and checksums. These remain pre-releases and never
become server/Play `latest`. Existing installer/updater URLs are unchanged; Mac
downloads use the releases list, not `latest/download`. Mac versions can trail
the server between batches: check publisher capabilities before using new
features, or dispatch manually. Mac capacity delays affect only Mac availability.
Published assets are immutable; there is no relabelling of old binaries with a
new server version. Scheduled workflows may need re-enabling after 60 days of
repository inactivity.

Both tracks use standard GitHub-hosted runners and no paid service. Linux
server publication still waits for Windows/Linux Play packaging to preserve
compatibility with existing clients that share GitHub's single latest channel.
Publication does not prove host installation: the updater may defer for active
games or roll back a failed activation. Verify the host build separately.

Server operators enable [automatic updates](deployment.md) separately.
