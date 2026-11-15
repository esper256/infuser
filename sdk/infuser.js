// Pre-1.0 Infuser browser SDK; see docs/sdk-contract.md before extending this contract.
// Import it, or include it as a module script for automatic heartbeats.
export class InfuserError extends Error {
  constructor(message, status) { super(message); this.status = status; }
}
async function request(path, options = {}) {
  const response = await fetch('/_infuser/' + path, { credentials: 'same-origin', ...options });
  const result = await response.json();
  if (!response.ok) throw new InfuserError(result.error || 'Infuser request failed', response.status);
  return result;
}
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
export const Infuser = { ready, saves, leave };
window.Infuser = Infuser;
// Do not release on visibilitychange/pagehide: refreshing or navigating levels is normal.
// When a tab closes, the short session lease expires without any unload dependency.
