// Browser IDs identify a model, not an individual controller's serial number.
export const layouts = Object.freeze({
  xbox: Object.freeze(['A', 'B', 'X', 'Y']),
  playstation: Object.freeze(['×', '○', '□', '△']),
  nintendo: Object.freeze(['B', 'A', 'Y', 'X'])
});
export function automaticLayout(id = '') {
  if (/dualsense|dualshock|playstation|054c[-:]|vendor:\s*054c/i.test(id)) return 'playstation';
  if (/nintendo|joy[ -]?con|057e[-:]|vendor:\s*057e/i.test(id)) return 'nintendo';
  return 'xbox';
}
export function connectedPads() {
  try { return Array.from(navigator.getGamepads?.() || []).filter(pad => pad?.connected); }
  catch { return []; }
}
const preferenceKey = id => 'infuser.gamepad.layout.' + id;
export function preference(id) {
  try {
    const saved = localStorage.getItem(preferenceKey(id));
    if (Object.hasOwn(layouts, saved)) return saved;
  } catch {}
  return 'auto';
}
export function savePreference(id, value) {
  if (value === 'auto') localStorage.removeItem(preferenceKey(id));
  else if (Object.hasOwn(layouts, value)) localStorage.setItem(preferenceKey(id), value);
}
export function nativeLayouts() {
  return Object.fromEntries(connectedPads().slice(0, 16).map(pad => {
    const saved = preference(pad.id);
    return [pad.index, saved === 'auto' ? automaticLayout(pad.id) : saved];
  }));
}
// The launch fragment stays client-side. Only include overrides for visible pads.
export function gamepadLaunchUrl(url) {
  const entries = [];
  for (const pad of connectedPads().slice(0, 16)) {
    const layout = preference(pad.id);
    if (layout !== 'auto') entries.push([pad.id, layout]);
  }
  if (!entries.length) return url;
  const target = new URL(url);
  const fragment = new URLSearchParams(target.hash.slice(1));
  fragment.set('pad-layouts', JSON.stringify(entries));
  target.hash = fragment.toString();
  return target.href;
}

let sessionOverrides;
function launchOverrides() {
  if (!sessionOverrides) {
    sessionOverrides = new Map();
    try {
      for (const [id, layout] of JSON.parse(sessionStorage.getItem('infuser.launch.pad-layouts') || '[]')) {
        if (typeof id === 'string' && Object.hasOwn(layouts, layout)) sessionOverrides.set(id, layout);
      }
    } catch {}
  }
  return sessionOverrides;
}
export const gamepads = Object.freeze({
  layout(pad) {
    return launchOverrides().get(pad?.id) || automaticLayout(pad?.id);
  },
  faceButton(pad, index) {
    if (!Number.isInteger(index) || index < 0 || index > 3) return null;
    return layouts[gamepads.layout(pad)][index];
  }
});
