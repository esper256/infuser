/* Runs in <head> after the base sheets, still before first paint.
 * Theme code owns presentation selection only: no game, profile, or API behavior.
 * Nightglass is linked by the page. Any other active theme is inserted here. */
(() => {
  const key = 'infuser.theme';
  const defaultId = 'nightglass';
  const catalog = Object.freeze([
    Object.freeze({id: 'nightglass', name: 'Nightglass', status: 'stable'}),
    Object.freeze({id: 'cartridge-club', name: 'Cartridge Club', status: 'stable'}),
    Object.freeze({id: 'funny-pages', name: 'Funny Pages', status: 'preview'}),
    Object.freeze({id: 'field-notes', name: 'Field Notes', status: 'preview'}),
    Object.freeze({id: 'castle-keep', name: 'Castle Keep', status: 'preview'}),
  ]);
  const resolve = id => catalog.find(theme => theme.id === id && theme.status !== 'retired')?.id || defaultId;
  const read = () => { try { return localStorage.getItem(key); } catch { return null; } };
  // Empty string: no extra sheet (Nightglass). Null: the registry has no sheet for this id.
  function hrefFor(id) {
    if (id === defaultId) return '';
    const registered = globalThis.InfuserThemeSheets;
    if (Array.isArray(registered)) {
      const sheet = registered.find(item => item && item.id === id && typeof item.href === 'string' && item.href);
      return sheet ? sheet.href : null;
    }
    return catalog.some(theme => theme.id === id) ? '/assets/themes/' + id + '.css' : null;
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
  function applyNow(id) {
    document.documentElement.dataset.theme = id;
    paintChrome();
    window.dispatchEvent(new Event('infuser-theme-change'));
  }
  let generation = 0;
  function remember(id) {
    try { localStorage.setItem(key, id); } catch { /* Still usable this visit. */ }
  }
  if (typeof document.addEventListener === 'function' && document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', paintChrome);
  }
  window.InfuserThemes = Object.freeze({
    catalog, defaultId,
    get current() { return document.documentElement.dataset.theme; },
    select(id) {
      const selected = resolve(id);
      const mine = ++generation;
      const href = hrefFor(selected);
      const finish = () => {
        if (mine !== generation) return;
        remember(selected);
        applyNow(selected);
      };
      if (href == null) return Promise.reject(new Error('theme'));
      if (!href || sheetReady(existingLink(selected))) { finish(); return Promise.resolve(); }
      return ensure(selected, href).then(finish);
    },
  });
  const initial = resolve(read());
  const initialHref = hrefFor(initial);
  if (initialHref) preloadInitial(initial, initialHref);
  applyNow(initialHref == null ? defaultId : initial);
  window.addEventListener('storage', event => {
    if (event.key !== key && event.key !== null) return;
    const selected = resolve(read());
    const mine = ++generation;
    const href = hrefFor(selected);
    const finish = () => { if (mine !== generation) return; applyNow(selected); };
    if (href == null) return;
    if (!href || sheetReady(existingLink(selected))) { finish(); return; }
    ensure(selected, href).then(finish, () => {});
  });
})();
