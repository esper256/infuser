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

// A small, isolated control for every browser engine. No iframe or permanent title bar.
ready.then(session => {
  if (document.getElementById('__infuser_controls')) return;
  const host = document.createElement('div'); host.id = '__infuser_controls';
  const shadow = host.attachShadow({ mode: 'open' });
  shadow.innerHTML = `<style>
    :host{position:fixed;right:12px;bottom:12px;z-index:2147483000;font:12px system-ui;color:#effff3;}
    .controls{display:flex;gap:3px;align-items:center;padding:4px;background:#0c1825dd;border:1px solid #93baaa55;border-radius:10px;box-shadow:0 3px 16px #0004;backdrop-filter:blur(8px)}
    button,a{font:inherit;cursor:pointer;color:#e7fff0;background:transparent;border:0;border-radius:6px;padding:8px;text-decoration:none;display:inline-block}
    button:hover,a:hover{background:#b3f0c322;color:#c5ffd8}button:focus-visible,a:focus-visible{outline:2px solid #c5ffd8;outline-offset:1px}
    .message{max-width:220px;margin:0 0 8px;background:#15283a;padding:10px;border-radius:8px;line-height:1.5}.message:empty{display:none}
    @media(max-width:600px){:host{right:8px;bottom:max(8px,env(safe-area-inset-bottom))}}@media(prefers-reduced-motion:no-preference){button:active{transform:scale(.94)}}
  </style><p class="message" role="status"></p><div class="controls"><a aria-label="Back to Infuser library" title="Back to library">↩</a><button type="button" aria-label="Enter fullscreen" title="Fullscreen">⛶ Fullscreen</button></div>`;
  shadow.querySelector('a').href = session.library_url;
  const button = shadow.querySelector('button'), message = shadow.querySelector('.message');
  button.addEventListener('click', async () => {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else if (document.documentElement.requestFullscreen) await document.documentElement.requestFullscreen();
      else { message.textContent='This browser does not support fullscreen for games. Try its Hide Toolbar or Add to Home Screen option.'; return; }
      message.textContent='';
    } catch { message.textContent='Fullscreen was blocked by the browser. Try opening this game in its own tab.'; }
  });
  document.addEventListener('fullscreenchange', () => {
    button.textContent=document.fullscreenElement?'⛶ Exit fullscreen':'⛶ Fullscreen';
    button.setAttribute('aria-label',document.fullscreenElement?'Exit fullscreen':'Enter fullscreen');
  });
  document.body.append(host);
}).catch(() => {});
