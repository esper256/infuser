// Pre-1.0 Infuser browser SDK; see docs/sdk-contract.md before extending this contract.
// Import it, or include it as a module script for automatic heartbeats.
import {sessionDenials, sessionFetch} from '/_infuser/sdk/v1/session-fetch.js';
import {cloud as checkpoints} from '/_infuser/sdk/v1/cloud.js';
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
  constructor(message, status) { super(typeof message === 'string' ? message : message?.message || 'Infuser request failed'); this.status = status; if(message?.code)this.code=message.code; if(message?.reason)this.reason=message.reason; }
}
async function request(path, options = {}) {
  const response = await sessionFetch('/_infuser/api/v1/' + path, { credentials: 'same-origin', ...options });
  if (!response.ok) {
    const result = await response.json().catch(() => null);
    throw new InfuserError(result?.error || 'Infuser request failed', response.status);
  }
  return response.json();
}
export const version = Object.freeze({major:1, apiMajor:1, status:'preview'});
export const ready = request('context');
// An embedding game can listen for infuser-session-error and show a return-to-Library action.
let stopped = false;
let updateAnnounced = false;
let clientVersion, forceReload;
function stopHeartbeat() {
  stopped = true;
  clearInterval(timer);
  globalThis.document?.removeEventListener('visibilitychange', onVisible);
  sessionDenials.removeEventListener('denied', onDenied);
}
function sessionError(error) {
  if (stopped) return;
  stopHeartbeat();
  if (window.parent && window.parent !== window) {
    window.parent.postMessage({type: 'infuser-session-error', status: error.status, message: error.message, reason: error.reason}, location.origin);
  }
  window.dispatchEvent(new CustomEvent('infuser-session-error', {detail: error}));
}
async function heartbeat() {
  if (stopped) return;
  try {
    const state = await request('heartbeat', { method: 'POST' });
    if (stopped || !window.parent || window.parent === window) return;
    const update = state.client_update;
    if (update && (update.version !== clientVersion || update.force_reload !== forceReload)) {
      clientVersion = update.version;
      forceReload = update.force_reload;
      window.parent.postMessage({type: 'infuser-client-update', version: clientVersion, force_reload: forceReload}, location.origin);
    }
    if (!updateAnnounced && state.update_available) {
      updateAnnounced = true;
      window.parent.postMessage({type: 'infuser-update-available'}, location.origin);
    }
  }
  catch { /* A failed heartbeat can run again on the normal schedule. */ }
}
function onVisible() {
  if (document.visibilityState === 'visible') void heartbeat();
}
function onDenied(event) {
  const {error, status} = event.detail;
  sessionError(new InfuserError(error || 'This game session ended.', status));
}
sessionDenials.addEventListener('denied', onDenied);
globalThis.document?.addEventListener('visibilitychange', onVisible);
ready.then(heartbeat, () => {});
const timer = setInterval(heartbeat, 25000);
// BroadcastChannel is scoped to this origin and browser storage partition.
const otherTabs = new Set();
let tabId, channel, dirty = false, pageHiding = false;
export const tabs = Object.freeze({get others() { return otherTabs.size; }});

function notifyTabs() {
  const detail = {others: tabs.others, unsaved: dirty};
  window.dispatchEvent(new CustomEvent('infuser-tabs', {detail}));
  if (window.parent && window.parent !== window) {
    window.parent.postMessage({type: 'infuser-tabs', ...detail}, location.origin);
  }
}

function startTab() {
  pageHiding = false;
  if (channel || !window.BroadcastChannel || !globalThis.crypto?.randomUUID) return;
  try { channel = new BroadcastChannel('infuser-tabs'); } catch { return; }
  tabId ??= crypto.randomUUID();
  channel.onmessage = ({data}) => {
    if (!data || typeof data.tab !== 'string' || data.tab === tabId) return;
    const before = otherTabs.size;
    if (data.type === 'bye') {
      otherTabs.delete(data.tab);
    } else if (data.type === 'hello') {
      // A new arrival also gives survivors a chance to forget a crashed tab.
      // Ask once for first-hand replies. A roster request never starts another.
      if (!data.roster) otherTabs.clear();
      otherTabs.add(data.tab);
      channel.postMessage({type: 'here', tab: tabId, unsaved: dirty});
      if (!data.roster) channel.postMessage({type: 'hello', tab: tabId, roster: true});
    } else if (data.type === 'here') {
      otherTabs.add(data.tab);
    }
    if (otherTabs.size !== before) notifyTabs();
  };
  channel.postMessage({type: 'hello', tab: tabId});
}

function stopTab() {
  pageHiding = true;
  if (!channel) return;
  channel.postMessage({type: 'bye', tab: tabId});
  channel.onmessage = null;
  channel.close();
  channel = null;
  if (otherTabs.size) { otherTabs.clear(); notifyTabs(); }
}
window.addEventListener('pagehide', stopTab);
window.addEventListener('pageshow', event => { if (event.persisted) startTab(); });
startTab();

export function onBeforeLeave(callback) {
  if (typeof callback !== 'function') throw new TypeError('onBeforeLeave requires a callback');
  const hidden = () => { if (document.visibilityState === 'hidden') callback('hidden'); };
  const hide = () => callback('pagehide');
  document.addEventListener('visibilitychange', hidden);
  window.addEventListener('pagehide', hide);
  return () => {
    document.removeEventListener('visibilitychange', hidden);
    window.removeEventListener('pagehide', hide);
  };
}

export const saves = {
  setDirty(value) {
    if (typeof value !== 'boolean') throw new TypeError('setDirty requires a boolean');
    if (dirty === value) return;
    dirty = value;
    channel?.postMessage({type: 'here', tab: tabId, unsaved: dirty});
    notifyTabs();
  },
  load: (slot = 'progress') => request('saves/' + encodeURIComponent(slot)),
  // Keep the returned version. 409 means reload/resolve; never blindly overwrite.
  save(slot, data, version) {
    const body = JSON.stringify({data, version});
    const options = {method: 'PUT', headers: {'Content-Type': 'application/json'}, body};
    if ((pageHiding || globalThis.document?.visibilityState === 'hidden') && new Blob([body]).size <= 65536) {
      options.keepalive = true;
    }
    return request('saves/' + encodeURIComponent(slot), options);
  }
};
export async function leave() { stopHeartbeat(); await request('heartbeat', { method: 'DELETE' }); }
// Loaded only by games that explicitly enter their peer-hosted multiplayer mode.
export const peers = {
  create: async options => (await import('/_infuser/sdk/v1/peers.js')).create(options),
  join: async options => (await import('/_infuser/sdk/v1/peers.js')).join(options)
};
export const peerjs = {
  create: async options => (await import('/_infuser/sdk/v1/peerjs.js')).create(options),
  join: async options => (await import('/_infuser/sdk/v1/peerjs.js')).join(options)
};
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
export const Infuser = { version, ready, saves, tabs, onBeforeLeave, leave, peers, peerjs, cloud, runtime, presence, launchIntent, sessions };
window.Infuser = Infuser;
// Do not release on visibilitychange/pagehide: refreshing or navigating levels is normal.
// When a tab closes, the short session lease expires without any unload dependency.
