# Dedicated server fixture

A small authoritative Node/ws simulation using Three.js vectors. It runs at 30 Hz,
sends 15 snapshots per second, and supports up to eight players per room. Both an
independent run and a shared run require the server. This is a private test fixture,
not Lanternfall's source or a claim that Lanternfall has been tested.

Standalone: run `npm ci --ignore-scripts` in `server`, then `node server.js` and open
`http://localhost:8080`. This path makes no Infuser requests.

Hosted: run `node examples/dedicated-node/prepare.mjs` from the repository root.
This stages `.local/dedicated-sdk-project` with the current server helper. Publish
that staged directory with the normal publisher and `--preview`. The agent
needs no Docker installation. The server's Node packaging recipe must be enabled
on the installation. The source and public export are separate. The title screen
stays cold until a run is chosen. `?autoplay=1` chooses the shared run and moves its
player deterministically; it uses no model or tokens.

The fixture exercises verified identity, guest isolation, multiple and single-player
runs, Join Friend, reconnects, idle shutdown and queued updates. It keeps the same
gameplay messages in standalone and hosted modes. Room labels are not passwords.

## Infuser contributor acceptance test

The commands below test changes to Infuser itself. They require its Rust development
environment and browser-test dependencies. **Game-making agents do not need this
harness, Rust, or a browser download to integrate or publish their own game.**
Use the source above as an example, run your project's existing checks, then follow
the [publishing verification instructions](../../docs/agents.md#publish-and-verify).
This harness is not currently shipped as a portable game-author tool.

For contributors testing locally without Google or Docker, stage it, run `npm ci --ignore-scripts`
in the staged `server` folder, then run:

```sh
INFUSER_TEST_NODE=node cargo run --example dedicated_fixture -- \
  .local/dedicated-sdk-project .local/dedicated-ready.json
node tests/dedicated_sessions_journey.mjs .local/dedicated-ready.json
```

The browser test requires Playwright from `services/peerjs` and an installed Chromium.
The harness binds only loopback, uses synthetic members and deletes temporary data
on exit. Its ready file contains a test publishing credential: keep it private.
Production tests still use the real Docker/Caddy deployment.
