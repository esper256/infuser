import {connectedPads, layouts} from './gamepad-layout.js';
export {automaticLayout, connectedPads, layouts} from './gamepad-layout.js';

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
// Snapshot the visible controllers while the Library still has focus.
export function captureGamepadLayouts() {
  try {
    const entries = [];
    for (const pad of connectedPads().slice(0, 16)) {
      const layout = preference(pad.id);
      if (layout !== 'auto') entries.push([pad.id, layout]);
    }
    if (entries.length) {
      const snapshot = JSON.stringify(entries);
      if (snapshot.length <= 32768) return snapshot;
    }
  } catch {} // Controller access is optional for launch.
  return null;
}

export function gamepadLaunchUrl(url, snapshot) {
  if (!snapshot) return url;
  try {
    const target = new URL(url);
    const fragment = new URLSearchParams(target.hash.slice(1));
    fragment.set('pad-layouts', snapshot);
    target.hash = fragment.toString();
    return target.href;
  } catch { return url; }
}
