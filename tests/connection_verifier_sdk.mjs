import {test} from 'node:test';
import assert from 'node:assert/strict';
import {generateKeyPairSync, sign} from 'node:crypto';
import {createConnectionVerifier} from '../sdk/server/connection.mjs';

test('server accepts only fresh instance-bound platform proofs, once; hosted failures never become standalone', () => {
  const {publicKey, privateKey} = generateKeyPairSync('ed25519');
  const env = {INFUSER_GAME_ID:'pond', INFUSER_INSTANCE_ID:'runtime-a', INFUSER_CONNECTION_ISSUER:'https://games.example.test', INFUSER_CONNECTION_PUBLIC_KEY:publicKey.export({format:'jwk'}).x};
  const verify = createConnectionVerifier(env);
  const now = Math.floor(Date.now()/1000);
  const identity = {iss:env.INFUSER_CONNECTION_ISSUER, aud:'infuser-server-connection', iat:now, exp:now+60, group_id:'home', app_id:'pond', build_id:'one', instance_id:'runtime-a', launch_id:'launch', connection_id:'connection', player_id:'player', name:'Player', guest:false};
  const proof = (changes={}, key=privateKey) => {
    const input = [JSON.stringify({alg:'EdDSA', typ:'infuser-connection+jwt', v:1}), JSON.stringify({...identity,...changes})].map(s => Buffer.from(s).toString('base64url')).join('.');
    return {headers:{'x-infuser-connection':input+'.'+sign(null,Buffer.from(input),key).toString('base64url')}};
  };
  assert.equal(createConnectionVerifier({}), null);
  assert.throws(() => createConnectionVerifier({INFUSER_GAME_ID:'pond'}), /Incomplete/);
  assert.throws(() => verify({headers:{}}), /Missing/);
  for (const change of [{iss:'https://other.test'}, {aud:'other'}, {app_id:'other'}, {instance_id:'retired'}, {iat:now+10}, {exp:now}, {exp:now+3600}, {guest:'false'}, {player_id:''}]) {
    assert.throws(() => verify(proof(change)), /scoped/);
  }
  assert.throws(() => verify(proof({},generateKeyPairSync('ed25519').privateKey)), /signature/);
  assert.deepEqual(verify(proof()), identity);
  assert.throws(() => verify(proof()), /already used/);
  assert.equal(verify(proof({connection_id:'guest-connection',player_id:'guest-launch',guest:true})).guest, true);
});
