# Dedicated server fixture

A small authoritative Node/ws simulation using Three.js vectors. It runs at 30 Hz,
sends 15 snapshots per second, and supports up to eight players per room. Both an
independent run and a shared run require the server. This is a private test fixture,
not Lanternfall's source or a claim that Lanternfall has been tested.

Standalone: run `npm ci --ignore-scripts` in `server`, then `node server.js` and open
`http://localhost:8080`. This path makes no Infuser requests.

Hosted: publish this directory with the normal publisher and `--preview`. The agent
needs no Docker installation. The server's Node packaging recipe must be enabled
on the installation. The source and public export are separate. The title screen
stays cold until a run is chosen. `?autoplay=1` chooses the shared run and moves its
player deterministically; it uses no model or tokens.

The fixture currently tests hosting and gameplay transport. Identity verification,
session discovery and Join Friend are separate integration steps; room labels in
this fixture are not passwords or access credentials.
