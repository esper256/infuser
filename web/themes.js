/* Runs in <head> after the base sheets, still before first paint.
 * Theme code owns presentation selection only: no game, profile, or API behavior.
 * Nightglass is linked by the page. Any other active theme is inserted here. */
(() => {
  const key = 'infuser.theme';
  const schemesKey = 'infuser.theme.schemes';
  const customKey = 'infuser.theme.custom';
  const lastKey = 'infuser.theme.last';
  const profilePrefix = 'infuser.appearance.';
  const sessionProfileKey = 'infuser.profile';
  const defaultId = 'nightglass';
  const holidayId = 'thanksgiving';
  const holidayTheme = Object.freeze({id: holidayId, name: 'Thankfuser', status: 'seasonal'});
  // U.S. Thanksgiving in each device's local calendar: the fourth Thursday.
  const isThanksgiving = date => date.getMonth() === 10 && date.getDay() === 4 && date.getDate() >= 22 && date.getDate() <= 28;
  let holidayActive = isThanksgiving(new Date());
  const defaultCustom = Object.freeze({primary: '#122128', accent: '#b5ead6', secondary: '#e8d5a4'});
  const hexColor = /^#[0-9a-f]{6}$/i;
  const nightglassSchemes = Object.freeze([
    Object.freeze({id: 'default', name: 'Smoked Mint (Default)', swatches: Object.freeze(['#080f15', '#122128', '#b5ead6', '#e8d5a4'])}),
    Object.freeze({id: 'deep-ocean', name: 'Deep Ocean', swatches: Object.freeze(['#07101c', '#102033', '#7dd3fc', '#fde68a'])}),
    Object.freeze({id: 'twilight-violet', name: 'Twilight Violet', swatches: Object.freeze(['#0d0a18', '#1b1430', '#c4b5fd', '#f9a8d4'])}),
    Object.freeze({id: 'amber-hearth', name: 'Amber Hearth', swatches: Object.freeze(['#140e09', '#251a11', '#fbbf24', '#fb923c'])}),
    Object.freeze({id: 'rose-quartz', name: 'Rose Quartz', swatches: Object.freeze(['#160b12', '#281522', '#fda4af', '#fcd34d'])}),
    Object.freeze({id: 'cobalt-pulse', name: 'Cobalt Pulse', swatches: Object.freeze(['#080d1e', '#121c38', '#60a5fa', '#67e8f9'])}),
    Object.freeze({id: 'forest-moss', name: 'Forest Moss', swatches: Object.freeze(['#09120c', '#14251a', '#86efac', '#fde047'])}),
    Object.freeze({id: 'crimson-velvet', name: 'Crimson Velvet', swatches: Object.freeze(['#16090c', '#291218', '#f87171', '#fbbf24'])}),
    Object.freeze({id: 'northern-aurora', name: 'Northern Aurora', swatches: Object.freeze(['#071316', '#0f252b', '#5eead4', '#c084fc'])}),
    Object.freeze({id: 'copper-forge', name: 'Copper Forge', swatches: Object.freeze(['#140d0a', '#261913', '#f59e6b', '#fcd34d'])}),
    Object.freeze({id: 'glacier-frost', name: 'Glacier Frost', swatches: Object.freeze(['#0a1118', '#152230', '#bae6fd', '#e2e8f0'])}),
    Object.freeze({id: 'sunset-dusk', name: 'Sunset Dusk', swatches: Object.freeze(['#150b16', '#271529', '#f472b6', '#fbbf24'])}),
    Object.freeze({id: 'neon-berry', name: 'Neon Berry', swatches: Object.freeze(['#120818', '#22102e', '#e879f9', '#38bdf8'])}),
    Object.freeze({id: 'obsidian-gold', name: 'Obsidian Gold', swatches: Object.freeze(['#0e0d0a', '#1d1a13', '#eab308', '#f5d08a'])}),
    Object.freeze({id: 'custom', name: 'Custom colors…', swatches: Object.freeze(['#080f15', '#122128', '#b5ead6', '#e8d5a4'])}),
  ]);
  const emptySchemes = Object.freeze([]);
  const catalog = Object.freeze([
    Object.freeze({id: 'nightglass', name: 'Nightglass', status: 'stable', schemes: nightglassSchemes}),
    Object.freeze({id: 'cartridge-club', name: 'Cartridge Club', status: 'stable'}),
    Object.freeze({id: 'funny-pages', name: 'Funny Pages', status: 'preview'}),
    Object.freeze({id: 'field-notes', name: 'Field Notes', status: 'preview'}),
    Object.freeze({id: 'castle-keep', name: 'Castle Keep', status: 'preview'}),
  ]);
  const customProps = Object.freeze([
    'color-scheme',
    '--page', '--field', '--detail-panel', '--panel', '--surface', '--feedback-bubble',
    '--line', '--ink', '--quiet', '--accent', '--accent-hover', '--accent-pressed',
    '--button-ink', '--green', '--presence-online', '--warm', '--focus', '--glass',
    '--card-dock', '--browser-chrome', '--page-glow-1', '--page-glow-2',
    '--secondary-bg', '--secondary-hover', '--add-bg', '--add-border',
    '--add-hover-bg', '--add-hover-border', '--header-line', '--search-bg',
    '--search-border', '--dock-border', '--dock-top-border', '--self-glow-1',
    '--self-glow-2', '--dialog-border', '--dialog-top-border', '--notice-bg',
    '--notice-border', '--own-bubble-tint', '--own-meta-ink', '--tag-pill-bg',
  ]);
  const resolve = id => catalog.find(theme => theme.id === id && theme.status !== 'retired')?.id || defaultId;
  const schemesOf = id => catalog.find(theme => theme.id === id && theme.status !== 'retired')?.schemes || emptySchemes;
  const resolveScheme = (themeId, schemeId) => {
    const schemes = schemesOf(themeId);
    if (!schemes.length) return '';
    return schemes.find(scheme => scheme.id === schemeId)?.id || schemes[0].id;
  };
  const readKey = storageKey => { try { return localStorage.getItem(storageKey); } catch { return null; } };
  function readJson(storageKey) {
    try {
      const raw = localStorage.getItem(storageKey);
      if (!raw) return null;
      const parsed = JSON.parse(raw);
      return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : null;
    } catch { return null; }
  }
  function writeKey(storageKey, value) {
    try { localStorage.setItem(storageKey, value); } catch { /* Still usable this visit. */ }
  }
  function writeJson(storageKey, value) {
    writeKey(storageKey, JSON.stringify(value));
  }
  function readSessionProfile() {
    try {
      const value = typeof sessionStorage !== 'undefined' ? sessionStorage.getItem(sessionProfileKey) : null;
      return typeof value === 'string' && value ? value : null;
    } catch { return null; }
  }
  const cleanHex = (value, fallback) => typeof value === 'string' && hexColor.test(value.trim()) ? value.trim().toLowerCase() : fallback;
  const cleanCustom = raw => ({
    primary: cleanHex(raw?.primary, defaultCustom.primary),
    accent: cleanHex(raw?.accent, defaultCustom.accent),
    secondary: cleanHex(raw?.secondary, defaultCustom.secondary),
  });
  function normalizeAppearance(raw) {
    const theme = resolve(raw?.theme);
    const schemes = {};
    if (raw?.schemes && typeof raw.schemes === 'object' && !Array.isArray(raw.schemes)) {
      for (const [themeId, schemeId] of Object.entries(raw.schemes)) {
        const known = resolve(themeId);
        if (known !== themeId || !schemesOf(known).length) continue;
        const resolved = resolveScheme(known, schemeId);
        if (resolved && resolved !== 'default') schemes[known] = resolved;
      }
    }
    const custom = {};
    if (raw?.custom && typeof raw.custom === 'object' && !Array.isArray(raw.custom)) {
      for (const [themeId, colors] of Object.entries(raw.custom)) {
        const known = resolve(themeId);
        if (known !== themeId || !schemesOf(known).some(scheme => scheme.id === 'custom')) continue;
        if (colors && typeof colors === 'object' && !Array.isArray(colors)) custom[known] = cleanCustom(colors);
      }
    }
    return {theme, schemes, custom};
  }
  function isNonDefault(appearance) {
    return appearance.theme !== defaultId || Object.keys(appearance.schemes).length > 0 || Object.keys(appearance.custom).length > 0;
  }
  function readLegacy() {
    const rawTheme = readKey(key);
    const rawSchemes = readJson(schemesKey);
    const rawCustom = readJson(customKey);
    if (rawTheme === null && rawSchemes === null && rawCustom === null) return null;
    return normalizeAppearance({theme: rawTheme, schemes: rawSchemes || {}, custom: rawCustom || {}});
  }
  const profileKey = id => profilePrefix + id;
  function readProfileCache(playerId) {
    if (!playerId) return null;
    const raw = readJson(profileKey(playerId));
    return raw ? normalizeAppearance(raw) : null;
  }
  function readLast() {
    const raw = readJson(lastKey);
    return raw ? normalizeAppearance(raw) : null;
  }
  let activeProfileId = readSessionProfile();
  function resolveInitialState() {
    if (activeProfileId) {
      return readProfileCache(activeProfileId) || readLegacy() || normalizeAppearance(null);
    }
    return readLast() || readLegacy() || normalizeAppearance(null);
  }
  const bootState = resolveInitialState();
  let preferredTheme = bootState.theme;
  let savedSchemes = {...bootState.schemes};
  let savedCustom = {...bootState.custom};
  function currentSnapshot(themeId = preferredTheme) {
    return normalizeAppearance({theme: themeId, schemes: savedSchemes, custom: savedCustom});
  }
  function persist(themeId = preferredTheme) {
    const snap = currentSnapshot(themeId);
    if (activeProfileId) {
      writeJson(profileKey(activeProfileId), snap);
      writeJson(lastKey, snap);
    } else {
      writeKey(key, snap.theme);
      writeJson(schemesKey, snap.schemes);
      writeJson(customKey, snap.custom);
    }
  }
  function schemeFor(themeId) {
    if (themeId === holidayId) return '';
    const id = resolve(themeId);
    return resolveScheme(id, savedSchemes[id]);
  }
  function customColorsFor(themeId) {
    const id = resolve(themeId);
    return cleanCustom(savedCustom[id]);
  }
  function hexToRgb(hex) {
    const n = parseInt(hex.slice(1), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }
  function rgbToHex(r, g, b) {
    const part = v => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0');
    return '#' + part(r) + part(g) + part(b);
  }
  function mix(a, b, amount) {
    const [ar, ag, ab] = hexToRgb(a), [br, bg, bb] = hexToRgb(b);
    return rgbToHex(ar + (br - ar) * amount, ag + (bg - ag) * amount, ab + (bb - ab) * amount);
  }
  function luminance(hex) {
    const [r, g, b] = hexToRgb(hex).map(c => {
      const s = c / 255;
      return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  }
  function contrastRatio(a, b) {
    const la = luminance(a), lb = luminance(b);
    return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
  }
  function contrastInk(hex) {
    return contrastRatio(hex, '#0c161b') >= contrastRatio(hex, '#f4f8fa') ? '#0c161b' : '#f4f8fa';
  }
  function customTokens({primary, accent, secondary}) {
    const baseLum = luminance(primary);
    const isLight = baseLum > 0.18;
    if (isLight) {
      const veryLight = baseLum > 0.5;
      const page = veryLight ? mix(primary, '#000000', 0.06) : mix(primary, '#ffffff', 0.12);
      const field = veryLight ? mix(primary, '#ffffff', 0.65) : mix(primary, '#ffffff', 0.42);
      const detail = veryLight ? mix(primary, '#ffffff', 0.35) : mix(primary, '#ffffff', 0.24);
      const surface = veryLight ? mix(primary, '#000000', 0.07) : mix(primary, '#ffffff', 0.22);
      const line = veryLight ? mix(primary, '#000000', 0.28) : mix(primary, '#000000', 0.36);
      const ink = veryLight ? mix('#111a1f', primary, 0.03) : '#080e12';
      const quiet = veryLight ? mix('#415058', primary, 0.05) : '#182228';
      const accentReadable = contrastRatio(accent, primary) >= 4.5;
      const focusColor = contrastRatio(secondary, primary) >= 3 ? secondary : mix(secondary, '#000000', 0.55);
      return {
        'color-scheme': 'light',
        '--page': page,
        '--field': field,
        '--detail-panel': detail,
        '--panel': primary,
        '--surface': surface,
        '--feedback-bubble': veryLight ? mix(primary, '#000000', 0.06) : mix(primary, '#ffffff', 0.3),
        '--line': line,
        '--ink': ink,
        '--quiet': quiet,
        '--accent': accent,
        '--accent-hover': mix(accent, '#000000', 0.14),
        '--accent-pressed': mix(accent, '#000000', 0.26),
        '--button-ink': contrastInk(accent),
        '--green': accent,
        '--presence-online': mix(accent, '#15803d', 0.55),
        '--warm': secondary,
        '--focus': focusColor,
        '--glass': 'linear-gradient(135deg, ' + mix(primary, '#ffffff', 0.25) + 'f4, ' + page + 'fa)',
        '--card-dock': 'linear-gradient(125deg, ' + mix(primary, '#ffffff', 0.3) + 'ec, ' + surface + 'f4)',
        '--browser-chrome': page,
        '--page-glow-1': accent + '26',
        '--page-glow-2': secondary + '22',
        '--secondary-bg': veryLight ? mix(primary, '#000000', 0.09) : mix(primary, '#ffffff', 0.32),
        '--secondary-hover': veryLight ? mix(primary, '#000000', 0.16) : mix(primary, '#ffffff', 0.46),
        '--add-bg': accentReadable ? mix(primary, accent, 0.12) : '#1b262c',
        '--add-border': accentReadable ? mix(primary, accent, 0.45) : '#1b262c',
        '--add-hover-bg': accentReadable ? mix(primary, accent, 0.22) : '#27363e',
        '--add-hover-border': accentReadable ? mix(accent, '#000000', 0.2) : '#27363e',
        '--header-line': line,
        '--search-bg': field,
        '--search-border': line,
        '--dock-border': line,
        '--dock-top-border': line,
        '--self-glow-1': accent + '28',
        '--self-glow-2': accent + '40',
        '--dialog-border': line,
        '--dialog-top-border': contrastRatio(accent, primary) >= 2.5 ? accent : mix(accent, '#000000', 0.35),
        '--notice-bg': field + 'f5',
        '--notice-border': line,
        '--own-bubble-tint': secondary,
        '--own-meta-ink': mix(secondary, '#000000', 0.5),
        '--tag-pill-bg': field + 'e8',
      };
    }
    const page = mix(primary, '#000000', 0.36);
    const field = mix(primary, '#000000', 0.22);
    const detail = mix(primary, '#000000', 0.14);
    const surface = mix(primary, '#ffffff', 0.08);
    const line = mix(primary, accent, 0.28);
    const ink = baseLum > 0.06 ? '#ffffff' : mix('#f3f6f8', accent, 0.07);
    const quiet = baseLum > 0.06 ? '#e5eef2' : mix('#a3b0b8', accent, 0.12);
    return {
      'color-scheme': 'dark',
      '--page': page,
      '--field': field,
      '--detail-panel': detail,
      '--panel': primary,
      '--surface': surface,
      '--feedback-bubble': mix(primary, accent, 0.12),
      '--line': line,
      '--ink': ink,
      '--quiet': quiet,
      '--accent': accent,
      '--accent-hover': mix(accent, '#ffffff', 0.22),
      '--accent-pressed': mix(accent, '#000000', 0.18),
      '--button-ink': contrastInk(accent),
      '--green': accent,
      '--presence-online': mix(accent, '#4ade80', 0.45),
      '--warm': secondary,
      '--focus': secondary,
      '--glass': 'linear-gradient(135deg, ' + mix(primary, accent, 0.1) + 'ee, ' + mix(primary, '#000000', 0.18) + 'f6)',
      '--card-dock': 'linear-gradient(125deg, ' + mix(primary, accent, 0.16) + '92, ' + mix(primary, '#000000', 0.32) + 'ec)',
      '--browser-chrome': page,
      '--page-glow-1': accent + '24',
      '--page-glow-2': secondary + '1a',
      '--secondary-bg': mix(primary, accent, 0.1),
      '--secondary-hover': mix(primary, accent, 0.2),
      '--add-bg': mix(primary, accent, 0.14),
      '--add-border': mix(primary, accent, 0.42),
      '--add-hover-bg': mix(primary, accent, 0.22),
      '--add-hover-border': mix(accent, '#ffffff', 0.1),
      '--header-line': accent + '1c',
      '--search-bg': field,
      '--search-border': line,
      '--dock-border': accent + '38',
      '--dock-top-border': accent + '5c',
      '--self-glow-1': accent + '18',
      '--self-glow-2': accent + '28',
      '--dialog-border': mix(primary, accent, 0.42),
      '--dialog-top-border': mix(accent, '#ffffff', 0.1),
      '--notice-bg': mix(primary, accent, 0.18) + 'f5',
      '--notice-border': mix(primary, accent, 0.55),
      '--own-bubble-tint': secondary,
      '--own-meta-ink': mix(secondary, '#ffffff', 0.25),
      '--tag-pill-bg': mix(primary, '#000000', 0.34) + 'cf',
    };
  }
  function clearCustomTokens() {
    const style = document.documentElement.style;
    if (!style || typeof style.removeProperty !== 'function') return;
    for (const prop of customProps) style.removeProperty(prop);
  }
  function applyCustomTokens(colors) {
    const style = document.documentElement.style;
    if (!style || typeof style.setProperty !== 'function') return;
    for (const [prop, value] of Object.entries(customTokens(colors))) style.setProperty(prop, value);
  }
  // Empty string: no extra sheet (Nightglass). Null: the registry has no sheet for this id.
  function hrefFor(id) {
    if (id === defaultId) return '';
    const registered = globalThis.InfuserThemeSheets;
    if (Array.isArray(registered)) {
      const sheet = registered.find(item => item && item.id === id && typeof item.href === 'string' && item.href);
      return sheet ? sheet.href : null;
    }
    return id === holidayId || catalog.some(theme => theme.id === id) ? '/assets/themes/' + id + '.css' : null;
  }
  function escapeAttr(value) {
    return String(value).replace(/[&"<>]/g, char => ({'&': '&amp;', '"': '&quot;', '<': '&lt;', '>': '&gt;'}[char]));
  }
  function existingLink(id) {
    if (typeof document.querySelector !== 'function') return null;
    return document.querySelector('link[data-infuser-theme="' + id + '"]');
  }
  function sheetReady(link) {
    return !!link && (link.dataset.loaded === 'true' || !!link.sheet);
  }
  function whenLoaded(link) {
    if (sheetReady(link)) { link.dataset.loaded = 'true'; return Promise.resolve(); }
    return new Promise((resolve, reject) => {
      let settled = false;
      const finish = () => { if (settled) return; settled = true; link.dataset.loaded = 'true'; resolve(); };
      const fail = () => {
        if (settled) return;
        settled = true;
        link.dataset.failed = 'true';
        if (typeof link.remove === 'function') link.remove();
        reject(new Error('theme'));
      };
      link.addEventListener('load', finish, {once: true});
      link.addEventListener('error', fail, {once: true});
      if (sheetReady(link)) finish();
    });
  }
  function createLink(id, href) {
    const link = document.createElement('link');
    link.rel = 'stylesheet';
    link.href = href;
    link.dataset.infuserTheme = id;
    link.setAttribute('blocking', 'render');
    return link;
  }
  function mount(link) {
    (document.head || document.documentElement).appendChild(link);
    return link;
  }
  function preloadInitial(id, href) {
    if (existingLink(id)) return;
    // A parser-blocking script can write a real stylesheet link. The parser
    // treats it as render-blocking, so the first paint already has this theme.
    if (document.readyState === 'loading' && typeof document.write === 'function') {
      document.write('<link rel="stylesheet" href="' + escapeAttr(href) + '" data-infuser-theme="' + escapeAttr(id) + '" blocking="render">');
      return;
    }
    if (typeof document.createElement === 'function') mount(createLink(id, href));
  }
  function ensure(id, href) {
    let found = existingLink(id);
    if (found && found.dataset.failed === 'true') {
      if (typeof found.remove === 'function') found.remove();
      found = null;
    }
    if (found) return whenLoaded(found);
    if (typeof document.createElement !== 'function') return Promise.reject(new Error('theme'));
    const link = createLink(id, href);
    const pending = whenLoaded(link);
    mount(link);
    return pending;
  }
  function paintChrome() {
    if (typeof getComputedStyle !== 'function' || typeof document.querySelector !== 'function') return;
    const meta = document.querySelector('meta[name="theme-color"]');
    if (!meta) return;
    const chrome = getComputedStyle(document.documentElement).getPropertyValue('--browser-chrome').trim();
    if (chrome) meta.content = chrome;
  }
  function applySchemeFor(id) {
    const scheme = schemeFor(id);
    if (!scheme || scheme === 'default') delete document.documentElement.dataset.themeScheme;
    else document.documentElement.dataset.themeScheme = scheme;
    if (scheme === 'custom') applyCustomTokens(customColorsFor(id));
    else clearCustomTokens();
  }
  const originalBrand = new WeakMap();
  function paintBrand() {
    if (typeof document.querySelectorAll !== 'function') return;
    for (const image of document.querySelectorAll('.brand-icon,.chooser-brand > img')) {
      if (!originalBrand.has(image)) originalBrand.set(image, image.getAttribute('src'));
      const src = holidayActive ? '/assets/themes/thanksgiving/turkey.png' : originalBrand.get(image);
      if (image.getAttribute('src') !== src) image.setAttribute('src', src);
    }
    const wordmark = document.querySelector('.brand-wordmark');
    if (wordmark) wordmark.setAttribute('aria-label', (holidayActive ? 'Thankfuser' : 'Infuser') + ' Library');
    const chooser = document.querySelector('.chooser-brand > span');
    if (chooser) chooser.textContent = holidayActive ? 'Thankfuser' : 'Infuser';
    if (/^(Infuser|Thankfuser)( ·|$)/.test(document.title)) document.title = document.title.replace(/^(Infuser|Thankfuser)/, holidayActive ? 'Thankfuser' : 'Infuser');
  }
  function applyNow(id) {
    preferredTheme = id;
    id = holidayActive ? holidayId : id;
    document.documentElement.dataset.theme = id;
    applySchemeFor(id);
    paintChrome();
    paintBrand();
    window.dispatchEvent(new Event('infuser-theme-change'));
  }
  let generation = 0;
  function applyState(state, shouldPersist = false) {
    const normalized = normalizeAppearance(state);
    const mine = ++generation;
    const effective = holidayActive ? holidayId : normalized.theme;
    const href = hrefFor(effective);
    const finish = () => {
      if (mine !== generation) return;
      savedSchemes = {...normalized.schemes};
      savedCustom = {...normalized.custom};
      if (shouldPersist) persist(normalized.theme);
      applyNow(normalized.theme);
    };
    if (href == null) return Promise.reject(new Error('theme'));
    if (!href || sheetReady(existingLink(effective))) { finish(); return Promise.resolve(); }
    return ensure(effective, href).then(finish);
  }
  if (typeof document.addEventListener === 'function' && document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => { paintChrome(); paintBrand(); });
  }
  let diagnosticAppearance;
  window.InfuserThemes = Object.freeze({
    get diagnosticAppearance(){return diagnosticAppearance;},
    get catalog() { return holidayActive ? Object.freeze([...catalog, holidayTheme]) : catalog; },
    get seasonalLocked() { return holidayActive; },
    refreshSeason,
    defaultId, defaultCustom,
    get current() { return document.documentElement.dataset.theme; },
    get currentScheme() { return schemeFor(document.documentElement.dataset.theme); },
    get profileId() { return activeProfileId; },
    snapshot() { return currentSnapshot(); },
    schemesFor(themeId = document.documentElement.dataset.theme) {
      return themeId === holidayId ? emptySchemes : schemesOf(resolve(themeId));
    },
    schemeFor(themeId = document.documentElement.dataset.theme) {
      return schemeFor(themeId);
    },
    customColorsFor(themeId = document.documentElement.dataset.theme) {
      return customColorsFor(themeId);
    },
    cacheProfile(playerId, appearance) {
      if (!playerId || !appearance || typeof appearance !== 'object') return;
      writeJson(profileKey(playerId), normalizeAppearance(appearance));
    },
    pruneProfiles(activeIds) {
      if (!Array.isArray(activeIds)) return;
      const keep = new Set(activeIds.filter(id => typeof id === 'string' && id));
      try {
        if (typeof localStorage.length !== 'number' || typeof localStorage.key !== 'function' || typeof localStorage.removeItem !== 'function') return;
        const stale = [];
        for (let i = 0; i < localStorage.length; i++) {
          const storageKey = localStorage.key(i);
          if (storageKey && storageKey.startsWith(profilePrefix) && !keep.has(storageKey.slice(profilePrefix.length))) {
            stale.push(storageKey);
          }
        }
        for (const storageKey of stale) localStorage.removeItem(storageKey);
      } catch { /* Ignore blocked storage. */ }
    },
    bindProfile(playerId, serverAppearance) {
      activeProfileId = typeof playerId === 'string' && playerId ? playerId : null;
      if (!activeProfileId) return Promise.resolve(null);
      let target;
      let migrated = null;
      if (serverAppearance && typeof serverAppearance === 'object') {
        target = normalizeAppearance(serverAppearance);
      } else {
        const legacy = readLegacy();
        if (legacy && isNonDefault(legacy)) {
          target = legacy;
          migrated = legacy;
        } else {
          target = normalizeAppearance(null);
        }
      }
      diagnosticAppearance={theme:target.theme,source:serverAppearance?'server':migrated?'legacy':'default'};
      return applyState(target, true).then(() => migrated, () => migrated);
    },
    select(id) {
      if (holidayActive) return Promise.resolve();
      const selected = resolve(id);
      const mine = ++generation;
      const href = hrefFor(selected);
      const finish = () => {
        if (mine !== generation) return;
        persist(selected);
        applyNow(selected);
      };
      if (href == null) return Promise.reject(new Error('theme'));
      if (!href || sheetReady(existingLink(selected))) { finish(); return Promise.resolve(); }
      return ensure(selected, href).then(finish);
    },
    selectScheme(schemeId, themeId = document.documentElement.dataset.theme) {
      if (holidayActive) return '';
      const target = resolve(themeId);
      const schemes = schemesOf(target);
      if (!schemes.length) return '';
      const resolved = resolveScheme(target, schemeId);
      if (resolved === 'default') delete savedSchemes[target];
      else savedSchemes[target] = resolved;
      persist();
      if (target === document.documentElement.dataset.theme) {
        applySchemeFor(target);
        paintChrome();
        window.dispatchEvent(new Event('infuser-theme-change'));
      }
      return resolved;
    },
    setCustomColors(colors, themeId = document.documentElement.dataset.theme) {
      if (holidayActive) return customColorsFor(preferredTheme);
      const target = resolve(themeId);
      const schemes = schemesOf(target);
      if (!schemes.some(scheme => scheme.id === 'custom')) return customColorsFor(target);
      const next = cleanCustom({...customColorsFor(target), ...(colors || {})});
      savedCustom[target] = next;
      savedSchemes[target] = 'custom';
      persist();
      if (target === document.documentElement.dataset.theme) {
        applySchemeFor(target);
        paintChrome();
        window.dispatchEvent(new Event('infuser-theme-change'));
      }
      return next;
    },
    resetCustomColors(themeId = document.documentElement.dataset.theme) {
      if (holidayActive) return customColorsFor(preferredTheme);
      const target = resolve(themeId);
      delete savedCustom[target];
      persist();
      if (target === document.documentElement.dataset.theme && schemeFor(target) === 'custom') {
        applySchemeFor(target);
        paintChrome();
        window.dispatchEvent(new Event('infuser-theme-change'));
      }
      return customColorsFor(target);
    },
  });
  const initialTheme = holidayActive ? holidayId : bootState.theme;
  const initialHref = hrefFor(initialTheme);
  if (initialHref) preloadInitial(initialTheme, initialHref);
  applyNow(initialHref == null ? defaultId : bootState.theme);
  // One midnight timeout, paused while hidden. Focus/visibility also handle sleep
  // and time-zone changes. The override never enters a saved appearance snapshot.
  let seasonTimer;
  async function refreshSeason() {
    if (typeof clearTimeout === 'function') clearTimeout(seasonTimer);
    const date = new Date();
    if (typeof setTimeout === 'function' && !document.hidden) {
      const midnight = new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1);
      seasonTimer = setTimeout(() => { void refreshSeason(); }, Math.max(1, midnight - date + 20));
    }
    const next = isThanksgiving(date);
    if (next === holidayActive) return;
    holidayActive = next;
    try { await applyState(currentSnapshot()); } catch { /* Retry loading on the next focus. */ holidayActive = !next; }
  }
  void refreshSeason();
  window.addEventListener('focus', () => { void refreshSeason(); });
  if (typeof document.addEventListener === 'function') document.addEventListener('visibilitychange', () => { void refreshSeason(); });
  window.addEventListener('storage', event => {
    if (activeProfileId) {
      if (event.key !== profileKey(activeProfileId) && event.key !== null) return;
      const next = readProfileCache(activeProfileId) || normalizeAppearance(null);
      diagnosticAppearance={theme:next.theme,source:'other_tab'};
      applyState(next, false).catch(() => {});
      return;
    }
    if (event.key !== lastKey && event.key !== key && event.key !== schemesKey && event.key !== customKey && event.key !== null) return;
    const next = readLast() || readLegacy() || normalizeAppearance(null);
    applyState(next, false).catch(() => {});
  });
})();
