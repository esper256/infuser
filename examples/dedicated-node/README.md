# Node/WebSocket integration example

Audience: game authors and their agents.

`--preview` in this fixture is for admin-only testing. Publish normally when an
ordinary user needs to play; hidden builds are not available to them.

A small authoritative Node/ws simulation using Three.js vectors. It runs at 30 Hz,
sends 15 snapshots per second, and supports up to eight players per room. Both an
independent run and a shared run require the server. Use it to see how the server helper fits into a game with existing room logic.

Standalone: run `npm ci --ignore-scripts` in `server`, then `node server.js` and open
`http://localhost:8080`. This path makes no Infuser requests.

Hosted: run `node examples/dedicated-node/prepare.mjs` from the repository root.
This stages `.local/dedicated-sdk-project` with the current server helper. Publish
that staged directory with the normal publisher and `--preview`. The agent
needs no Docker installation. The server's Node packaging recipe must be enabled
on the installation. The source and public export are separate. The title screen
stays cold until a run is chosen. `?autoplay=1` chooses the shared run and moves its
player deterministically; it uses no model or tokens.

The example demonstrates verified identity, guest isolation, multiple and single-player
runs, Join Friend, reconnects, idle shutdown and queued updates. It keeps the same
gameplay messages in standalone and hosted modes. Room labels are not passwords.
