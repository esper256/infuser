# Enable managed Node hosting

Audience: server operators.

The operator stages each supported Node base once and puts its repository digest in
the private controller configuration. Game authors never perform this host step.
For example, pull `node:24-bookworm-slim` and inspect its `RepoDigests`; do not store
an unpinned tag in configuration. Node 22 is also supported when configured.

```toml
[controller.node_packaging]
images = { "24" = "node@sha256:REPLACE_WITH_STAGED_IMAGE_DIGEST" }
image_budget_mb = 2048
```

Upgrade the controller and apply the `X-Infuser-Upstream` routing in
[`deploy/Caddyfile.example`](../deploy/Caddyfile.example) before adding this table.
Strip client-supplied routing headers before forward authentication. Validate and
reload Caddy, run `infuser check`, then restart the controller.
The digest must identify a Linux image for the host architecture. The worker verifies
the actual Node major too. The controller service needs its existing Docker access;
Node is not installed on the host. Docker 25+ is needed for readiness probe intervals.

The existing CPU/RAM budget must accommodate a 768 MiB / 500 millicore packaging
reservation. Packaging waits for room in that budget. At least 2 GiB of temporary
working space must remain above the disk reserve. Source limits are 8 MiB per file,
64 MiB and 4,096 files per build; registry downloads are bounded separately.
The image budget conservatively counts each retained image's reported size,
including shared base layers. Remove unused builds through the publisher API to
release their images; no global Docker prune is used. Keep this configuration guard
when rolling back: older binaries cannot safely manage unfinished source jobs.

npm's [offline and script settings](https://docs.npmjs.com/cli/using-npm/config/)
and Docker's [runtime isolation options](https://docs.docker.com/reference/cli/docker/container/run/)
describe these container settings.
