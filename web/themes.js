/* Runs before CSS to restore the local preference without a flash of another theme.
 * Theme code owns presentation selection only: no game, profile, or API behavior. */
(() => {
  const key = 'infuser.theme';
  const defaultId = 'nightglass';
  const catalog = Object.freeze([
    Object.freeze({id: 'nightglass', name: 'Nightglass', status: 'stable'}),
    Object.freeze({id: 'cartridge-club', name: 'Cartridge Club', status: 'preview'}),
  ]);
  const resolve = id => catalog.find(theme => theme.id === id && theme.status !== 'retired')?.id || defaultId;
  const read = () => { try { return localStorage.getItem(key); } catch { return null; } };
  function apply(id) {
    document.documentElement.dataset.theme = resolve(id);
    window.dispatchEvent(new Event('infuser-theme-change'));
  }
  window.InfuserThemes = Object.freeze({
    catalog, defaultId,
    get current() { return document.documentElement.dataset.theme; },
    select(id) {
      const selected = resolve(id);
      apply(selected);
      try { localStorage.setItem(key, selected); } catch { /* Still usable this visit. */ }
    },
  });
  apply(read());
  window.addEventListener('storage', event => {
    if (event.key === key || event.key === null) apply(read());
  });
})();
