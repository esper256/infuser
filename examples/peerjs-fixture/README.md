# PeerJS Flight Test

Audience: Infuser developers.

Small disposable networking fixture, not Event Horizon. It reuses one real
PeerJS DataConnection handler for hosted and standalone play, including binary
messages and automatic motion without AI. Keep it an admin-only preview.

`tests/peerjs_journey.mjs` serves the source directly through the loopback fixture.
For live acceptance, stage these files in an ignored export directory:

- `index.html`, `game.js`, and `infuser.game.json` from this folder.
- `sdk/optional.js` as `infuser-platform.js`.
- `services/peerjs/node_modules/peerjs/dist/peerjs.min.js` as `peerjs.min.js`, after
  `npm ci --prefix services/peerjs`. Preserve its bundled copyright/license notice.

Publish the staged directory with a private connection file and `--preview`.
The publisher adds the versioned SDK to the uploaded HTML; keep the source HTML
unchanged so standalone mode has no platform dependency. `?autoplay=1&relay=1`
starts the hosted forced-relay test. This fixture intentionally has no persistent
progress; a real game must separately complete its saves/guest acceptance.
