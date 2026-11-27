// Browser IDs identify a model, not an individual controller's serial number.
export const layouts = Object.freeze({
  xbox: Object.freeze(['A', 'B', 'X', 'Y']),
  playstation: Object.freeze(['×', '○', '□', '△']),
  nintendo: Object.freeze(['B', 'A', 'Y', 'X'])
});
export function automaticLayout(id = '') {
  // Firefox starts with vendor-product. Chromium includes a named vendor field.
  const vendor = /^([\da-f]{1,4})-/i.exec(id)?.[1]
    || /\bvendor:\s*([\da-f]{4})\b/i.exec(id)?.[1]
    || /\(([\da-f]{4})-[\da-f]{4}\)$/i.exec(id)?.[1];
  const vendorId = Number.parseInt(vendor, 16);
  if (vendorId === 0x054c || /\b(?:dualsense|dualshock|playstation)\b|\bps[45] controller\b/i.test(id)) return 'playstation';
  if (vendorId === 0x057e || /\bnintendo\b|\bjoy[ -]?con\b/i.test(id)) return 'nintendo';
  return 'xbox';
}
export function connectedPads() {
  try { return Array.from(navigator.getGamepads?.() || []).filter(pad => pad?.connected); }
  catch { return []; }
}
function validEntry(entry) {
  return Array.isArray(entry) && entry.length === 2 &&
    typeof entry[0] === 'string' && typeof entry[1] === 'string' && Object.hasOwn(layouts, entry[1]);
}

let sessionOverrides;
function launchOverrides() {
  if (!sessionOverrides) {
    sessionOverrides = new Map();
    try {
      const raw = sessionStorage.getItem('infuser.launch.pad-layouts');
      if (raw && raw.length <= 32768) {
        const entries = JSON.parse(raw);
        if (Array.isArray(entries) && entries.length <= 16 && entries.every(validEntry)) {
          sessionOverrides = new Map(entries);
        }
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
    if (pad?.mapping !== 'standard' || !Number.isInteger(index) || index < 0 || index > 3) return null;
    return layouts[gamepads.layout(pad)][index];
  }
});
