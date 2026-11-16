// Internal SDK candidate. Not yet a supported game integration; see the implementation plan.
import {createPublicKey, verify} from 'node:crypto';

export function createConnectionVerifier(env = process.env) {
  const names = ['INFUSER_GAME_ID', 'INFUSER_INSTANCE_ID', 'INFUSER_CONNECTION_PUBLIC_KEY', 'INFUSER_CONNECTION_ISSUER'];
  if (names.every(name => !env[name])) return null; // Explicit standalone mode.
  if (names.some(name => !env[name])) throw new Error('Incomplete Infuser connection configuration');
  const key = createPublicKey({format:'jwk', key:{kty:'OKP', crv:'Ed25519', x:env.INFUSER_CONNECTION_PUBLIC_KEY}});
  const consumed = new Map();
  return function verifyConnection(request) {
    const proof = request.headers['x-infuser-connection'];
    if (typeof proof !== 'string' || proof.length > 8192) throw new Error('Missing or oversized Infuser connection proof');
    const parts = proof.split('.');
    if (parts.length !== 3 || parts.some(p => !/^[A-Za-z0-9_-]+$/.test(p))) throw new Error('Malformed Infuser connection proof');
    const header = JSON.parse(Buffer.from(parts[0], 'base64url'));
    if (header.alg !== 'EdDSA' || header.typ !== 'infuser-connection+jwt' || header.v !== 1
      || !verify(null, Buffer.from(parts.slice(0, 2).join('.')), key, Buffer.from(parts[2], 'base64url'))) {
      throw new Error('Invalid Infuser connection signature');
    }
    const identity = JSON.parse(Buffer.from(parts[1], 'base64url'));
    const now = Math.floor(Date.now() / 1000);
    if (identity.iss !== env.INFUSER_CONNECTION_ISSUER || identity.aud !== 'infuser-server-connection'
      || identity.app_id !== env.INFUSER_GAME_ID || identity.instance_id !== env.INFUSER_INSTANCE_ID
      || !Number.isSafeInteger(identity.iat) || !Number.isSafeInteger(identity.exp)
      || identity.iat > now || identity.exp <= now || identity.exp <= identity.iat || identity.exp - identity.iat > 60
      || typeof identity.guest !== 'boolean'
      || ['group_id', 'build_id', 'launch_id', 'connection_id', 'player_id', 'name'].some(name => typeof identity[name] !== 'string' || !identity[name])) {
      throw new Error('Expired or incorrectly scoped Infuser connection proof');
    }
    for (const [id, expiry] of consumed) if (expiry <= now) consumed.delete(id);
    if (consumed.has(identity.connection_id)) throw new Error('Infuser connection proof already used');
    // Fail closed instead of evicting an unexpired replay marker under pressure.
    if (consumed.size >= 4096) throw new Error('Infuser connection verifier is at capacity');
    consumed.set(identity.connection_id, identity.exp);
    return Object.freeze(identity);
  };
}
