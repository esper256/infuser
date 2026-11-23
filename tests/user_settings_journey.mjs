// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs INFUSER_CHROMIUM=/path/to/chromium node tests/user_settings_journey.mjs
import assert from 'node:assert/strict';
import {readFile, mkdir} from 'node:fs/promises';
const {chromium} = await import(process.env.INFUSER_PLAYWRIGHT || 'playwright');
const root = new URL('../', import.meta.url);
const shotDir = process.env.INFUSER_SETTINGS_SCREENSHOTS || '';
const browser = await chromium.launch({headless: true, executablePath: process.env.INFUSER_CHROMIUM});

const me = {
  player_id: 'self',
  name: 'Alex',
  avatar: 'circuit-fox',
  avatar_prompt_seen: true,
  admin: true,
  profile_count: 2,
  group: {id: 'home', name: 'Home'},
  appearance: null,
  avatars: [
    {id: 'circuit-fox', name: 'Circuit Fox'},
    {id: 'moss-knight', name: 'Moss Knight'},
  ],
};
const apps = [
  {
    id: 'starlight-drift',
    title: 'Starlight Drift',
    description: 'Drift across glowing moons with friends.',
    tags: ['racing', 'co-op'],
    updated: Math.floor(Date.now() / 1000) - 3600,
    author: {id: 'self', name: 'Alex', avatar: 'circuit-fox'},
    reactions: [{id: 'r1', name: 'Sam', emoji: '🚀', text: 'So fast!', updated: Math.floor(Date.now() / 1000) - 1800}],
    players: [],
    join_targets: [],
  },
  {
    id: 'lantern-keep',
    title: 'Lantern Keep',
    description: 'Build cozy towers before dusk.',
    tags: ['puzzle', 'cozy'],
    updated: Math.floor(Date.now() / 1000) - 7200,
    author: {id: 'sam', name: 'Sam', avatar: 'moss-knight'},
    reactions: [],
    players: [],
    join_targets: [],
  },
];
const friends = [
  {id: 'self', name: 'Alex', avatar: 'circuit-fox', status: 'active'},
  {id: 'sam', name: 'Sam', avatar: 'moss-knight', status: 'active'},
];
const tags = [
  {id: 'racing', label: 'Racing', count: 1},
  {id: 'co-op', label: 'Co-op', count: 1},
  {id: 'puzzle', label: 'Puzzle', count: 1},
  {id: 'cozy', label: 'Cozy', count: 1},
];

try {
  const context = await browser.newContext({viewport: {width: 1280, height: 860}, serviceWorkers: 'block'});
  await context.route('https://library.test/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/api/me') return route.fulfill({json: me});
    if (path === '/api/profile/appearance') {
      if (route.request().method() === 'PUT') {
        me.appearance = route.request().postDataJSON();
        return route.fulfill({json: {appearance: me.appearance}});
      }
      return route.fulfill({json: {appearance: me.appearance}});
    }
    if (path === '/api/library') return route.fulfill({json: {apps, friends, tags}});
    if (path === '/api/chat') return route.fulfill({json: {messages: []}});
    if (path === '/api/voice') return route.fulfill({json: {configured: true, available: true, participants: [], self: null, settings: {}}});
    if (path === '/api/voice/settings') return route.fulfill({json: {}});
    if (path === '/api/visits') return route.fulfill({json: {enabled: false}});
    if (path === '/api/feedback') return route.fulfill({json: {entries: []}});
    if (path === '/api/client-updates') return route.fulfill({json: {}});
    let file = path === '/library' ? 'web/index.html' : path.startsWith('/assets/') ? 'web/' + path.slice(8) : null;
    const avatar = /^\/assets\/avatars\/([^/]+)\/(\d+)\.webp$/.exec(path);
    if (avatar) file = `assets/avatars/${avatar[1]}-${avatar[2]}.webp`;
    if (file) {
      try {
        const body = await readFile(new URL(file, root));
        const contentType = file.endsWith('.js') ? 'text/javascript'
          : file.endsWith('.css') ? 'text/css'
          : file.endsWith('.svg') ? 'image/svg+xml'
          : file.endsWith('.webp') ? 'image/webp'
          : file.endsWith('.woff2') ? 'font/woff2'
          : 'text/html';
        return route.fulfill({body, contentType});
      } catch {}
    }
    return route.fulfill({status: 404, body: ''});
  });

  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', err => errors.push(err.message));

  await page.goto('https://library.test/library');
  await page.locator('#account-trigger').waitFor();
  await page.locator('.game-open').first().waitFor();

  // 1. Profile menu stays clean: User settings is present, moved controls are absent, Sign out is last.
  await page.locator('#account-trigger').click();
  const menu = page.locator('.profile-menu .account-menu');
  assert.equal(await menu.locator('#open-settings').count(), 1);
  assert.equal(await menu.locator('#theme-picker').count(), 0, 'theme picker moved out of profile menu');
  assert.equal(await menu.locator('#open-profile').count(), 0, 'profile button moved out of profile menu');
  assert.equal(await menu.locator('#open-voice-settings').count(), 0, 'voice settings moved out of profile menu');
  assert.equal(await menu.evaluate(el => el.lastElementChild?.classList.contains('sign-out')), true, 'Sign out stays at the bottom');

  // 2. Open User settings modal.
  await page.locator('#open-settings').click();
  const dialog = page.locator('#user-settings');
  await dialog.waitFor({state: 'visible'});
  assert.equal(await page.locator('.profile-menu').evaluate(el => el.open), false, 'opening settings closes profile menu');

  // Measure library card & dialog geometry before changing Nightglass color schemes.
  const baseCardBox = await page.locator('.game').first().boundingBox();
  const baseHeaderBox = await page.locator('header').boundingBox();
  assert.equal(await page.locator('#theme-scheme-group').isVisible(), true, 'Nightglass reveals color scheme picker');
  assert.ok(await page.locator('#theme-scheme-picker option').count() >= 14, 'many Nightglass schemes available');
  assert.equal(await page.locator('#theme-custom-colors').isVisible(), false, 'custom pickers stay hidden for presets');
  assert.equal(await page.locator('#settings-profile-summary').innerText().then(t => t.includes('Alex')), true);

  // Default Nightglass colors for shared controls remain unchanged when no scheme is active.
  const defaultColors = await page.evaluate(() => {
    const tb = document.createElement('button');
    tb.className = 'text-button';
    document.body.append(tb);
    const textButton = getComputedStyle(tb).color;
    tb.remove();
    const notice = getComputedStyle(document.getElementById('notice')).color;
    const close = getComputedStyle(document.getElementById('close-settings')).color;
    return {textButton, notice, close};
  });
  assert.deepEqual(defaultColors, {
    textButton: 'rgb(180, 201, 201)',
    notice: 'rgb(224, 243, 232)',
    close: 'rgb(178, 201, 196)',
  }, 'default Nightglass colors stay unchanged');

  // Voice settings section appears inside User settings when voice is configured.
  await page.locator('#settings-voice-region').waitFor({state: 'visible'});
  assert.equal(await page.locator('#open-voice-settings').isVisible(), true, 'Voice settings button visible when configured');

  if (shotDir) {
    await mkdir(shotDir, {recursive: true});
    await page.emulateMedia({reducedMotion: 'reduce'});
    await page.screenshot({path: `${shotDir}/01-nightglass-default.png`});
  }

  // 3. Switch Nightglass color schemes: colors change, layout does not move.
  const defaultAccent = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--accent').trim());
  await page.locator('#theme-scheme-picker').selectOption('twilight-violet');
  const violetAccent = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--accent').trim());
  assert.notEqual(violetAccent, defaultAccent, 'scheme updates accent token');
  assert.equal(await page.evaluate(() => document.documentElement.dataset.themeScheme), 'twilight-violet');

  const violetCardBox = await page.locator('.game').first().boundingBox();
  const violetHeaderBox = await page.locator('header').boundingBox();
  assert.deepEqual(violetCardBox, baseCardBox, 'Nightglass scheme changes no card geometry');
  assert.deepEqual(violetHeaderBox, baseHeaderBox, 'Nightglass scheme changes no header geometry');

  if (shotDir) await page.screenshot({path: `${shotDir}/02-nightglass-violet.png`});

  await page.locator('#theme-scheme-picker').selectOption('amber-hearth');
  assert.equal(await page.evaluate(() => document.documentElement.dataset.themeScheme), 'amber-hearth');
  if (shotDir) await page.screenshot({path: `${shotDir}/03-nightglass-amber.png`});

  // 4. Custom color picker reveals 3 color inputs (>= 44px touch targets) and updates live.
  await page.locator('#theme-scheme-picker').selectOption('custom');
  await page.locator('#theme-custom-colors').waitFor({state: 'visible'});
  for (const id of ['#custom-color-primary', '#custom-color-accent', '#custom-color-secondary']) {
    const box = await page.locator(id).boundingBox();
    assert.ok(box && box.height >= 44 && box.width >= 44, `${id} should be at least 44x44px`);
  }
  await page.locator('#custom-color-primary').fill('#102030');
  await page.locator('#custom-color-accent').fill('#ff77aa');
  await page.locator('#custom-color-secondary').fill('#ffd166');
  const customTokens = await page.evaluate(() => ({
    panel: getComputedStyle(document.documentElement).getPropertyValue('--panel').trim(),
    accent: getComputedStyle(document.documentElement).getPropertyValue('--accent').trim(),
    warm: getComputedStyle(document.documentElement).getPropertyValue('--warm').trim(),
  }));
  assert.deepEqual(customTokens, {panel: '#102030', accent: '#ff77aa', warm: '#ffd166'});
  if (shotDir) await page.screenshot({path: `${shotDir}/04-nightglass-custom.png`});

  // 5. Switching to non-scheme themes hides color scheme sub-options & clears scheme styling.
  for (const [themeId, shotName] of [
    ['field-notes', '05-field-notes.png'],
    ['funny-pages', '06-funny-pages.png'],
    ['castle-keep', '07-castle-keep.png'],
    ['cartridge-club', '08-cartridge-club.png'],
  ]) {
    await page.locator('#theme-picker').selectOption(themeId);
    await page.waitForFunction(id => document.documentElement.dataset.theme === id, themeId);
    assert.equal(await page.locator('#theme-scheme-group').isVisible(), false, `${themeId} hides Nightglass schemes`);
    assert.equal(await page.evaluate(() => document.documentElement.dataset.themeScheme), undefined);
    assert.equal(await page.evaluate(() => document.documentElement.style.getPropertyValue('--accent')), '');
    assert.equal(await dialog.evaluate(d => d.scrollWidth <= d.clientWidth), true, `${themeId} dialog has no horizontal overflow`);
    if (shotDir) await page.screenshot({path: `${shotDir}/${shotName}`});
  }

  // 6. Switching back to Nightglass restores the saved custom scheme and pickers.
  await page.locator('#theme-picker').selectOption('nightglass');
  await page.waitForFunction(() => document.documentElement.dataset.theme === 'nightglass');
  assert.equal(await page.locator('#theme-scheme-group').isVisible(), true);
  assert.equal(await page.locator('#theme-scheme-picker').inputValue(), 'custom');
  assert.equal(await page.locator('#theme-custom-colors').isVisible(), true);
  assert.equal(await page.locator('#custom-color-accent').inputValue(), '#ff77aa');

  // Reset custom colors restores defaults cleanly.
  await page.locator('#reset-custom-colors').click();
  assert.equal(await page.locator('#custom-color-accent').inputValue(), '#b5ead6');

  // Persist a curated scheme across reload.
  await page.locator('#theme-scheme-picker').selectOption('northern-aurora');
  await page.reload();
  await page.locator('#account-trigger').waitFor();
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'nightglass');
  assert.equal(await page.evaluate(() => document.documentElement.dataset.themeScheme), 'northern-aurora');

  // 7. Mobile viewport (360x780) layout check: compact custom picker leaves Profile visible above the fold.
  await page.setViewportSize({width: 360, height: 780});
  await page.locator('#account-trigger').click();
  await page.locator('#open-settings').click();
  await dialog.waitFor({state: 'visible'});
  await page.locator('#theme-scheme-picker').selectOption('custom');
  await page.locator('#theme-custom-colors').waitFor({state: 'visible'});
  assert.equal(await dialog.evaluate(d => d.scrollWidth <= d.clientWidth), true, 'mobile dialog has no horizontal overflow');
  const mobileFold = await page.evaluate(() => {
    const panel = document.querySelector('#user-settings .settings-panel').getBoundingClientRect();
    const profile = document.getElementById('settings-profile-region').getBoundingClientRect();
    return {panelBottom: panel.bottom, profileTop: profile.top};
  });
  assert.ok(mobileFold.profileTop < mobileFold.panelBottom, 'Profile section stays visible above the fold on mobile');
  if (shotDir) await page.screenshot({path: `${shotDir}/09-mobile-custom.png`});

  // 8. Voice settings button closes User settings and opens the voice mixer.
  await page.locator('#open-voice-settings').click();
  await dialog.waitFor({state: 'hidden'});
  await page.locator('#voice-mixer:popover-open').waitFor({state: 'visible'});
  await page.keyboard.press('Escape');

  // 9. Reopen and verify Escape returns focus to #account-trigger.
  await page.locator('#account-trigger').click();
  await page.locator('#open-settings').click();
  await dialog.waitFor({state: 'visible'});
  await page.keyboard.press('Escape');
  await dialog.waitFor({state: 'hidden'});
  await page.waitForFunction(() => document.activeElement?.id === 'account-trigger');

  assert.deepEqual(errors, []);
  console.log('PASS: User settings dialog, Nightglass color schemes, custom color pickers, persistence, theme switching, and mobile layout.');
} finally {
  await browser.close();
}
