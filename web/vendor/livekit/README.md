# LiveKit browser SDK

Audience: Infuser developers.

`livekit-client.esm.mjs` is the unmodified bundled ESM distribution from
`livekit-client@2.24.1` (Apache-2.0), served locally and loaded only when joining
voice. No CDN or runtime package installation is used.

Source: https://github.com/livekit/client-sdk-js
Package: https://registry.npmjs.org/livekit-client/-/livekit-client-2.24.1.tgz
Package integrity: `sha512-8kkpTPuszlkza99XOV/QZVQG7M730i5gH52j0AZzsUMQpQfI9tEgW2JfDge7+Z2ZUUuBEtV3r2MYbALHFJ2HIg==`

When updating, verify the registry's package integrity, copy this bundle and its
license, and run the actual voice journey. Infuser deliberately selects the
supported dual-peer-connection signaling protocol (`singlePeerConnection:false`);
changes to paths, token refresh, query options or reconnect behavior need review.
