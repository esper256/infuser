// Pre-1.0 Infuser browser SDK; see docs/sdk-contract.md before extending this contract.
// Import it, or include it as a module script for automatic heartbeats.
import {cloud as checkpoints} from '/_infuser/sdk/v1/cloud.js';
export class InfuserError extends Error {
  constructor(message, status) { super(typeof message === 'string' ? message : message?.message || 'Infuser request failed'); this.status = status; if(message?.code)this.code=message.code; }
}
async function request(path, options = {}) {
  const response = await fetch('/_infuser/api/v1/' + path, { credentials: 'same-origin', ...options });
  const result = await response.json();
  if (!response.ok) throw new InfuserError(result.error || 'Infuser request failed', response.status);
  return result;
}
export const version = Object.freeze({major:1, apiMajor:1, status:'preview'});
export const ready = request('context');
// An embedding game can listen for infuser-session-error and show a return-to-Library action.
let stopped = false;
async function heartbeat() {
  if (stopped) return;
  try { await request('heartbeat', { method: 'POST' }); }
  catch (error) { window.dispatchEvent(new CustomEvent('infuser-session-error', { detail: error })); }
}
ready.then(heartbeat).catch(error => window.dispatchEvent(new CustomEvent('infuser-session-error', { detail: error })));
const timer = setInterval(heartbeat, 25000);
export const saves = {
  load: (slot = 'progress') => request('saves/' + encodeURIComponent(slot)),
  // Keep the returned version. 409 means reload/resolve; never blindly overwrite.
  save: (slot, data, version) => request('saves/' + encodeURIComponent(slot), {
    method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ data, version })
  })
};
export async function leave() { stopped = true; clearInterval(timer); await request('heartbeat', { method: 'DELETE' }); }
// Loaded only by games that explicitly enter their peer-hosted multiplayer mode.
export const peers = { join: async options => (await import('/_infuser/sdk/v1/peers.js')).join(options) };
export const peerjs = { join: async options => (await import('/_infuser/sdk/v1/peerjs.js')).join(options) };
const checkpoint = method => async (...args) => checkpoints[method](...args);
export const cloud = {
  load:checkpoint('load'),read:checkpoint('read'),history:checkpoint('history'),commit:checkpoint('commit'),resolve:checkpoint('resolve'),
  // Conversion is synchronous, matching the standalone cloud module and guide.
  encodeBinary:checkpoints.encodeBinary,decodeBinary:checkpoints.decodeBinary
};
// Managed backends only. Peer-hosted games continue to use peers.join().
// acquire() returns starting/ready; poll status() before connecting. Keep the
// returned session_id for release so an old retry cannot end a newer session.
export const runtime = {
  acquire: (options = {}) => request('runtime/v1', {method:'POST',signal:options.signal}),
  status: (options = {}) => request('runtime/v1', {signal:options.signal}),
  release: sessionId => {
    if (typeof sessionId !== 'string' || !/^[a-f0-9]{32}$/.test(sessionId)) {
      return Promise.reject(new TypeError('release requires the acquired session_id'));
    }
    return request('runtime/v1/' + sessionId, {method:'DELETE'});
  }
};
export const presence = {
  // A snapshot only: no backend acquisition, peer lease, or presence spoofing.
  get: () => request('presence/v1', {signal:AbortSignal.timeout(5000)}),
  setActivity: activity => request('presence/v1/activity', {method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({activity}),signal:AbortSignal.timeout(5000)})
};
export const launchIntent = {
  // Consume in the game after loading its state, never in the embedding player.
  // At most once: refresh and lost-response retries cannot silently replay a join.
  consume: () => request('launch-intent/v1/consume', {method:'POST',signal:AbortSignal.timeout(5000)})
};
export const sessions = {
  connect: async options => (await import('/_infuser/sdk/v1/sessions.js')).connect(options),
  disconnect: async socket => (await import('/_infuser/sdk/v1/sessions.js')).disconnect(socket)
};
export const Infuser = { version, ready, saves, leave, peers, peerjs, cloud, runtime, presence, launchIntent, sessions };
window.Infuser = Infuser;
// Do not release on visibilitychange/pagehide: refreshing or navigating levels is normal.
// When a tab closes, the short session lease expires without any unload dependency.
