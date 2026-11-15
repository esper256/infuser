# CI and dependency maintenance

PR checks are advisory: fast Rust journeys (`cargo test --locked`), Node 24 SDK
and theme tests (`node --test tests/*_sdk.mjs tests/themes.mjs`), Rust formatting
(`cargo fmt --all -- --check`), and Clippy
(`cargo clippy --locked --all-targets -- -D warnings`). Each check runs independently
with `continue-on-error`; inspect job/step results even if the overall check is green.
There is no approval environment, required check, or release dependency added here.
The release workflow remains separately maintained.

The **Docker and Caddy integration (manual)** workflow runs only via Actions →
Run workflow, once this workflow exists on the default branch. It builds the demo
image, runs the ignored Docker lifecycle journey, and runs the ignored Caddy web
journey with Caddy 2.10.2. It does not deploy or publish an image. See
[local integration commands](operations.md#verification) for the same test entry points.

Dependabot checks Cargo, GitHub Actions and the demo Dockerfile every Monday.
Version updates are grouped into one PR per ecosystem, with at most one open
version-update PR per ecosystem. Security updates have their own catch-all groups;
they are event-driven, not constrained by the weekly version-update schedule or
its PR limit. These rules take effect on the default branch. There is no auto-merge.

The standalone **Dependency audit (advisory)** workflow runs weekly and manually,
using cargo-audit 0.21.2 and the current RustSec database. Findings and operational
errors appear in the Actions job summary and warning annotations. It neither opens
individual issues nor blocks releases. This checks Cargo.lock, not container OS
packages or the code of Actions.

Native Dependabot alerts, the dependency graph and automated security-update PRs
are repository settings. This change does not enable or alter them; security groups
apply if those features are enabled. A repository administrator can review those
settings separately. See [GitHub's security-update guidance](https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/secure-your-dependencies/configure-security-updates).

Actions in the new workflows are pinned to full commit SHAs, with readable version
comments. The Rust installer action is pinned while the installed stable Rust
channel intentionally follows stable. Dependabot maintains action pins.
