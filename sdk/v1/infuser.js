// Pre-1.0 Infuser browser SDK; see docs/sdk-contract.md before extending this contract.
// Import it, or include it as a module script for automatic heartbeats.
export class InfuserError extends Error {
  constructor(message, status) { super(message); this.status = status; }
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
const checkpoint = method => async (...args) => (await import('/_infuser/sdk/v1/cloud.js')).cloud[method](...args);
export const cloud = {load:checkpoint('load'),read:checkpoint('read'),history:checkpoint('history'),commit:checkpoint('commit'),resolve:checkpoint('resolve')};
// Managed backends only. Peer-hosted games continue to use peers.join().
// acquire() returns starting/ready; poll status() before connecting. Keep the
// returned session_id for release so an old retry cannot end a newer session.
export const runtime = {
  acquire: () => request('runtime/v1', {method:'POST'}),
  status: () => request('runtime/v1'),
  release: sessionId => {
    if (typeof sessionId !== 'string' || !/^[a-f0-9]{32}$/.test(sessionId)) {
      return Promise.reject(new TypeError('release requires the acquired session_id'));
    }
    return request('runtime/v1/' + sessionId, {method:'DELETE'});
  }
};
export const presence = {
  // A snapshot only: no backend acquisition, peer lease, or presence spoofing.
  get: () => request('presence/v1', {signal:AbortSignal.timeout(5000)})
};
export const launchIntent = {
  // Consume in the game after loading its state, never in the embedding player.
  // At most once: refresh and lost-response retries cannot silently replay a join.
  consume: () => request('launch-intent/v1/consume', {method:'POST',signal:AbortSignal.timeout(5000)})
};
export const Infuser = { version, ready, saves, leave, peers, cloud, runtime, presence, launchIntent };
window.Infuser = Infuser;
// Do not release on visibilitychange/pagehide: refreshing or navigating levels is normal.
// When a tab closes, the short session lease expires without any unload dependency.
