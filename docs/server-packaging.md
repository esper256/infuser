# Managed Node server packaging

**Preview contract for the first dedicated-server slice.** Check the installation's
advertised capabilities before using it. The runtime SDK remains a preview. This adds a publishing recipe;
it does not redefine player, save, gameplay-session or lease identities.

An agent publishes the browser export and a separate server source folder as one
immutable build. The initial recipe accepts plain Node JavaScript, a package.json,
and an npm lockfile (version 2 or 3). It does not run author-supplied Dockerfiles,
build commands or install scripts. Dependencies requiring native installation need
a future recipe or the existing operator-staged image route.

The installation advertises supported Node majors and configured digest-pinned
runtime images. The agent selects a supported major rather than guessing a host
architecture. The first acceptance case uses Node, Three.js and ws, with an
authoritative server supporting single-player and multiplayer.

## Publishing contract

Build creation accepts either the existing `server` image configuration or a new
`server_recipe`, never both. The recipe declares `runtime: "node"`, `node_major`,
`entry`, `health_path`, and bounded runtime settings. Server files upload separately
from web files and are never served from the game's public artifact directory.

- `PUT .../builds/BUILD/server/files/PATH` stores a server source file.
- `POST .../builds/BUILD/server-build` seals the inputs and queues packaging.
- `GET .../builds/BUILD/server-build` returns state, bounded logs and an error.
- The existing publish endpoint activates only a successfully checked build.

The build ID is the idempotency key. Repeating finalization returns the existing
job. Source and client artifacts cannot change after finalization. A failed attempt
can be retried with identical inputs; fixing those inputs requires a new build ID.
An interrupted worker reports interruption explicitly and can be retried. A failed
build never replaces the currently published game.

States are `uploading`, `queued`, `packaging`, `checking`, `ready`, and `failed`.
Only the game-scoped publisher can inspect its source job and logs. No global job
listing is exposed. Infuser gives the worker no publishing or runtime credentials.
Treat logs as private game material: game code can print anything it contains.

## Execution and storage

Rust validates lockfile package URLs and integrity hashes, then fetches bounded
tarballs from the approved npm registry without redirects. Package installation
runs offline in one non-root container with no Docker socket, no platform secrets,
no game volumes, a read-only root filesystem, bounded temporary storage, CPU/RAM
limits and a deadline. npm lifecycle scripts are disabled. Repository npm config,
Git/URL/local dependencies and workspace links are rejected in this first recipe.

Image assembly copies the bounded prepared files into an unstarted container and
commits its filesystem with platform-selected settings. It does not invoke Docker
build or create a build cache. No project Dockerfile or build command runs.
A separate disposable container checks readiness
without network access or live game data. The resulting local image ID, base image,
lockfile hash and recipe version are recorded with the build.

Managed servers use a separate internal Docker network. There is no Internet
egress or route to cloud metadata; the installation hostname resolves to the
bridge's host gateway so the server can reach Infuser's HTTPS API. The controller's
private API remains bound to host loopback. This recipe does not support external
backend services. Existing operator-staged images retain their declared network
policy. The packaging worker and readiness probe have no networking at all.

One worker runs at a time. Its reservation participates in the controller's host
resource accounting. A restart cleans up owned packaging containers before releasing
that reservation. Disk reserve and image retention limits apply; active and retained
build images are not pruned to make a new build succeed.

## Acceptance

Publish from a machine without Docker, using a fixture with ws and Three.js. Cover
upload retries, mismatched lockfiles, unsafe dependency URLs, disabled install
scripts, worker interruption, readiness failure, capacity queuing and cleanup.
Upgrade a single-player game to single-player plus dedicated multiplayer under the
same app ID. Verify client/server pairing, two clients, a guest, cold title-screen
startup, idle shutdown and queued updates. Preserve the current game on every
failed publication. Later session discovery and Join Friend use the separate
[SDK decisions](sdk-1.0-decisions.md).

## Installation setup

The operator stages each supported Node base once and puts its repository digest in
the private controller configuration. Game authors never perform this host step.
For example, pull `node:24-bookworm-slim` and inspect its `RepoDigests`; do not store
an unpinned tag in configuration. Node 22 is also supported when configured.

```toml
[controller.node_packaging]
images = { "24" = "node@sha256:REPLACE_WITH_STAGED_IMAGE_DIGEST" }
image_budget_mb = 2048
```

Upgrade the controller before adding this table, run `infuser check`, then restart.
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
describe the primitives used here; the fixture must also verify the assembled path.
