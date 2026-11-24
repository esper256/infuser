// Pre-1.0 Infuser browser SDK; see docs/sdk-contract.md before extending this contract.
// Import it, or include it as a module script for automatic heartbeats.
if (typeof window !== 'undefined' && typeof HTMLCanvasElement !== 'undefined' && !window.infuserGraphicsTuned) {
  window.infuserGraphicsTuned = true;
  const dpr = Object.getOwnPropertyDescriptor(window, 'devicePixelRatio') || Object.getOwnPropertyDescriptor(Object.getPrototypeOf(window), 'devicePixelRatio');
  const getDpr = dpr?.get ? () => dpr.get.call(window) : () => window.devicePixelRatio || 1;
  try {
    Object.defineProperty(window, 'devicePixelRatio', {
      configurable: true,
      get() {
        const raw = getDpr() || 1;
        let capped = true;
        try { if (typeof window.parent?.infuserHdCap === 'function') capped = window.parent.infuserHdCap(); } catch {}
        if (!capped) return raw;
        const area = Math.max(1, (window.innerWidth || 1920) * (window.innerHeight || 1080));
        return Math.min(raw, Math.max(0.5, Math.sqrt((1920 * 1080) / area)));
      }
    });
  } catch {}
  const getContext = HTMLCanvasElement.prototype.getContext;
  if (getContext && !getContext.infuserWrapped) {
    const wrapped = function(type, attributes) {
      if ((type === 'webgl' || type === 'webgl2') && (!attributes || attributes.powerPreference === undefined)) {
        attributes = { ...attributes, powerPreference: 'high-performance' };
      }
      return getContext.call(this, type, attributes);
    };
    wrapped.infuserWrapped = true;
    HTMLCanvasElement.prototype.getContext = wrapped;
  }
}
export class InfuserError extends Error {
  constructor(message, status) { super(message); this.status = status; }
}
async function request(path, options = {}) {
  const response = await fetch('/_infuser/' + path, { credentials: 'same-origin', ...options });
  if (!response.ok) {
    const result = await response.json().catch(() => null);
    throw new InfuserError(result?.error || 'Infuser request failed', response.status);
  }
  return response.json();
}
export const ready = request('context');
// An embedding game can listen for infuser-session-error and show a return-to-Library action.
let stopped = false;
function stopHeartbeat() {
  stopped = true;
  clearInterval(timer);
}
function sessionError(error) {
  if (stopped) return;
  if ([401, 403, 404, 410].includes(error.status)) {
    stopHeartbeat();
    // Window events stay inside the iframe. Forward only the terminal status to the player.
    if (window.parent && window.parent !== window) {
      window.parent.postMessage({type: 'infuser-session-error', status: error.status}, location.origin);
    }
  }
  window.dispatchEvent(new CustomEvent('infuser-session-error', {detail: error}));
}
async function heartbeat() {
  if (stopped) return;
  try { await request('heartbeat', { method: 'POST' }); }
  catch (error) { sessionError(error); }
}
ready.then(heartbeat, sessionError);
const timer = setInterval(heartbeat, 25000);
export const saves = {
  load: (slot = 'progress') => request('saves/' + encodeURIComponent(slot)),
  // Keep the returned version. 409 means reload/resolve; never blindly overwrite.
  save: (slot, data, version) => request('saves/' + encodeURIComponent(slot), {
    method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ data, version })
  })
};
export async function leave() { stopHeartbeat(); await request('heartbeat', { method: 'DELETE' }); }
// Loaded only by games that explicitly enter their peer-hosted multiplayer mode.
export const peers = { join: async options => (await import('/_infuser/peers.js')).join(options) };
const checkpoint = method => async (...args) => (await import('/_infuser/cloud-v1.js')).cloud[method](...args);
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
export const Infuser = { ready, saves, leave, peers, cloud, runtime, presence, launchIntent };
window.Infuser = Infuser;
// Do not release on visibilitychange/pagehide: refreshing or navigating levels is normal.
// When a tab closes, the short session lease expires without any unload dependency.
