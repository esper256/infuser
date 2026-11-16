# Managed Node server packaging

Audience: game authors and their agents.

**Preview API for managed Node servers.** Check the installation's advertised
capabilities before using it. For the usual CLI workflow, start with the
[Node recipe](engines/node-server.md).

An agent publishes the browser export and a separate server source folder as one
immutable build. This recipe accepts plain Node JavaScript, a package.json,
and an npm lockfile (version 2 or 3). It does not run author-supplied Dockerfiles,
build commands or install scripts. Dependencies requiring native installation need
the advanced image route, with an image prepared by the operator.

The installation advertises supported Node majors and configured digest-pinned
runtime images. The agent selects a supported major rather than guessing a host
architecture.

## Publishing contract

Build creation accepts either the existing `server` image configuration or a new
`server_recipe`, never both. The recipe declares `runtime: "node"`, `node_major`,
`entry`, `health_path`, and bounded runtime settings. Server files upload separately
from web files and are never served from the game's public artifact directory.

- `PUT .../builds/BUILD/server/files/PATH` stores a server source file.
- `POST .../builds/BUILD/server-build` seals the inputs and queues packaging.
- `GET .../builds/BUILD/server-build` returns state, bounded logs and an error.
- The existing publish endpoint activates only a successfully checked build.

Repeating finalization for the same build ID returns the existing
job. Source and client artifacts cannot change after finalization. A failed attempt
can be retried with identical inputs; fixing those inputs requires a new build ID.
An interrupted worker reports interruption explicitly and can be retried. A failed
build never replaces the currently published game.

States are `uploading`, `queued`, `packaging`, `checking`, `ready`, and `failed`.
Only the game-scoped publisher can inspect its source job and logs.
Treat logs as private game material: game code can print anything it contains.

## Runtime limits

Dependencies must come from npm lockfiles v2/v3. Installation runs offline with
lifecycle scripts disabled. Native compilation, Git/local/URL dependencies,
workspaces, and project npm configuration are unsupported.

The server must start and pass its health check without credentials or live game
data. At runtime it can reach the provided Infuser API but cannot reach external
backend services or the internet. This recipe has no persistent world volume.
Use [dedicated sessions](dedicated-sessions.md) for verified player identity,
room discovery, reconnects, and Join Friend.
