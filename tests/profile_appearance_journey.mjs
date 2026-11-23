// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs INFUSER_CHROMIUM=/path/to/chromium node tests/profile_appearance_journey.mjs
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const {chromium} = await import(process.env.INFUSER_PLAYWRIGHT || 'playwright');
const root = new URL('../', import.meta.url);
const browser = await chromium.launch({headless: true, executablePath: process.env.INFUSER_CHROMIUM});

const profiles = {
  pHome: {
    player_id: 'pHome',
    name: 'Home Alex',
    avatar: 'circuit-fox',
    avatar_prompt_seen: true,
    admin: true,
    profile_count: 2,
    group: {id: 'home', name: 'Home Family'},
    home_group: 'home',
    home_group_name: 'Home Family',
    appearance: null,
    avatars: [{id: 'circuit-fox', name: 'Circuit Fox'}, {id: 'moss-knight', name: 'Moss Knight'}],
  },
  pAway: {
    player_id: 'pAway',
    name: 'Away Alex',
    avatar: 'moss-knight',
    avatar_prompt_seen: true,
    admin: false,
    profile_count: 2,
    group: {id: 'away', name: 'Cousins Club'},
    home_group: 'away',
    home_group_name: 'Cousins Club',
    appearance: null,
    avatars: [{id: 'circuit-fox', name: 'Circuit Fox'}, {id: 'moss-knight', name: 'Moss Knight'}],
  },
};

const appearanceWrites = [];

try {
  const context = await browser.newContext({viewport: {width: 1200, height: 840}, serviceWorkers: 'block'});
  await context.route('https://library.test/**', async route => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    const selected = request.headers()['x-infuser-profile'];

    if (path === '/api/profiles') {
      return route.fulfill({
        json: {
          profiles: Object.values(profiles).map(p => ({
            player_id: p.player_id,
            name: p.name,
            avatar: p.avatar,
            group: p.group,
            appearance: p.appearance,
          })),
        },
      });
    }
    if (path === '/api/me') {
      const active = selected && profiles[selected];
      if (!active) {
        return route.fulfill({
          status: 409,
          headers: {'x-infuser-profile-required': '1'},
          json: {error: 'Choose your profile.'},
        });
      }
      const visit = request.headers()['x-infuser-visit'];
      const inspect = request.headers()['x-infuser-inspect-group'];
      if (visit) {
        return route.fulfill({json: {...active, group: {id: 'away', name: 'Cousins Club'}}});
      }
      if (inspect) {
        return route.fulfill({json: {...active, inspecting: true, group: {id: 'away', name: 'Cousins Club'}}});
      }
      return route.fulfill({json: active});
    }
    if (path === '/api/profile/appearance') {
      const active = selected && profiles[selected];
      if (!active) return route.fulfill({status: 401, json: {error: 'Sign in.'}});
      if (request.method() === 'PUT') {
        const body = request.postDataJSON();
        appearanceWrites.push({profile: selected, body});
        active.appearance = body;
      }
      return route.fulfill({json: {appearance: active.appearance}});
    }
    if (path === '/api/library') {
      return route.fulfill({json: {apps: [], friends: [], tags: []}});
    }
    if (path === '/api/chat') return route.fulfill({json: {messages: []}});
    if (path === '/api/voice') return route.fulfill({json: {configured: true, available: true, participants: [], self: null, settings: {}}});
    if (path === '/api/voice/settings') return route.fulfill({json: {}});
    if (path === '/api/visits') return route.fulfill({json: {enabled: false}});
    if (path === '/api/client-updates') return route.fulfill({json: {}});

    let file = path === '/library' ? 'web/index.html'
      : path === '/profiles' ? 'web/profiles.html'
      : path.startsWith('/assets/') ? 'web/' + path.slice(8)
      : null;
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

  // Seed a per-device mic/speaker selection and a stale deleted-profile cache entry before entering the chooser.
  await page.goto('https://library.test/profiles');
  await page.locator('.profile-choice').first().waitFor();
  const initialDevices = {inputId: 'usb-mic-42', outputId: 'desk-speakers-7'};
  await page.evaluate(devices => {
    localStorage.setItem('infuser.voice.devices.v1', JSON.stringify(devices));
    localStorage.setItem('infuser.appearance.pDeleted', JSON.stringify({theme: 'castle-keep', schemes: {}, custom: {}}));
  }, initialDevices);
  await page.reload();
  await page.locator('.profile-choice').first().waitFor();
  assert.equal(await page.evaluate(() => localStorage.getItem('infuser.appearance.pDeleted')), null, 'deleted profile cache is pruned on /profiles');

  // 1. Enter Profile A (Home Family): starts on default Nightglass, shows group name above Visits in menu & Profile settings.
  await page.getByRole('button', {name: /Home Alex/}).click();
  await page.waitForURL('**/library');
  await page.locator('#account-trigger').waitFor();
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'nightglass');
  assert.equal(await page.evaluate(() => document.documentElement.dataset.themeScheme), undefined);

  await page.locator('#account-trigger').click();
  assert.equal(await page.locator('#account-group').isVisible(), true, 'group name shown in profile menu for 2+ profiles');
  assert.match(await page.locator('#account-group').innerText(), /Home Family/);
  const menuOrder = await page.locator('.profile-menu .account-menu').evaluate(menu => {
    const kids = [...menu.children].map(el => el.id);
    return {group: kids.indexOf('account-group'), visits: kids.indexOf('visits-button')};
  });
  assert.ok(menuOrder.group >= 0 && menuOrder.visits > menuOrder.group, 'group name sits above Visits in profile menu');
  await page.locator('#open-settings').click();
  await page.locator('#user-settings').waitFor({state: 'visible'});
  assert.match(await page.locator('#settings-profile-summary').innerText(), /Home Family/);
  assert.match(await page.locator('#settings-appearance-scope').innerText(), /Home Family/);
  assert.match(await page.locator('#settings-pwa').innerText(), /this device only/i);
  await page.locator('#settings-voice-region').waitFor({state: 'visible'});
  assert.match(await page.locator('#settings-voice').innerText(), /this device/i);

  // Pick Field Notes for Profile A and wait for server PUT.
  const putA = page.waitForResponse(r => new URL(r.url()).pathname === '/api/profile/appearance' && r.request().method() === 'PUT');
  await page.locator('#theme-picker').selectOption('field-notes');
  await putA;
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'field-notes');
  assert.equal(profiles.pHome.appearance.theme, 'field-notes');
  await page.locator('#close-settings').click();

  // 2. Switch Profile A -> /profiles chooser (keeps browser's last theme: field-notes) -> Profile B (Cousins Club).
  await page.locator('#account-trigger').click();
  await page.locator('#switch-profile').click();
  await page.waitForURL('**/profiles*');
  await page.locator('.profile-choice').first().waitFor();
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'field-notes', '/profiles uses browser last theme');

  await page.getByRole('button', {name: /Away Alex/}).click();
  await page.waitForURL('**/library');
  await page.locator('#account-trigger').waitFor();
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'nightglass', 'Profile B starts at default Nightglass without copying Profile A');
  assert.equal(await page.evaluate(() => document.documentElement.dataset.themeScheme), undefined);

  // Verify group name & pick Nightglass + Amber Hearth for Profile B.
  await page.locator('#account-trigger').click();
  assert.match(await page.locator('#account-group').innerText(), /Cousins Club/);
  await page.locator('#open-settings').click();
  await page.locator('#user-settings').waitFor({state: 'visible'});
  assert.match(await page.locator('#settings-profile-summary').innerText(), /Cousins Club/);
  const putB = page.waitForResponse(r => new URL(r.url()).pathname === '/api/profile/appearance' && r.request().method() === 'PUT');
  await page.locator('#theme-scheme-picker').selectOption('amber-hearth');
  await putB;
  assert.equal(await page.evaluate(() => document.documentElement.dataset.themeScheme), 'amber-hearth');
  await page.locator('#close-settings').click();

  // 3. Switch Profile B -> A: chooser uses last theme (amber-hearth), Profile A restores Field Notes immediately.
  await page.locator('#account-trigger').click();
  await page.locator('#switch-profile').click();
  await page.waitForURL('**/profiles*');
  await page.locator('.profile-choice').first().waitFor();
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'nightglass');
  assert.equal(await page.evaluate(() => document.documentElement.dataset.themeScheme), 'amber-hearth');

  await page.getByRole('button', {name: /Home Alex/}).click();
  await page.waitForURL('**/library');
  await page.locator('#account-trigger').waitFor();
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'field-notes', 'Profile A restores its own look');

  // 4. Switch Profile A -> B again: Profile B restores Amber Hearth, and mic/speaker choice is unchanged.
  await page.locator('#account-trigger').click();
  await page.locator('#switch-profile').click();
  await page.waitForURL('**/profiles*');
  await page.getByRole('button', {name: /Away Alex/}).click();
  await page.waitForURL('**/library');
  await page.locator('#account-trigger').waitFor();
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'nightglass');
  assert.equal(await page.evaluate(() => document.documentElement.dataset.themeScheme), 'amber-hearth');
  const devicesAfter = await page.evaluate(() => JSON.parse(localStorage.getItem('infuser.voice.devices.v1')));
  assert.deepEqual(devicesAfter, initialDevices, 'mic and speaker choice stays per device across switches');

  // 5. Open a second tab on Profile A and a third tab on Profile B: changing Profile A syncs only same-profile tabs.
  const tabA = await context.newPage();
  await tabA.goto('https://library.test/profiles');
  await tabA.getByRole('button', {name: /Home Alex/}).click();
  await tabA.waitForURL('**/library');
  await tabA.locator('#account-trigger').waitFor();
  assert.equal(await tabA.evaluate(() => document.documentElement.dataset.theme), 'field-notes');

  const tabA2 = await context.newPage();
  await tabA2.goto('https://library.test/profiles');
  await tabA2.getByRole('button', {name: /Home Alex/}).click();
  await tabA2.waitForURL('**/library');
  await tabA2.locator('#account-trigger').waitFor();

  await tabA.locator('#account-trigger').click();
  await tabA.locator('#open-settings').click();
  const putA2 = tabA.waitForResponse(r => new URL(r.url()).pathname === '/api/profile/appearance' && r.request().method() === 'PUT');
  await tabA.locator('#theme-picker').selectOption('funny-pages');
  await putA2;

  await tabA2.waitForFunction(() => document.documentElement.dataset.theme === 'funny-pages');
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'nightglass', 'Profile B tab ignores Profile A change');
  assert.equal(await page.evaluate(() => document.documentElement.dataset.themeScheme), 'amber-hearth');

  // 6. Visiting another group still names the home group in Appearance & Profile copy.
  const visitTab = await context.newPage();
  await visitTab.goto('https://library.test/profiles');
  await visitTab.evaluate(() => {
    sessionStorage.setItem('infuser.profile', 'pHome');
    sessionStorage.setItem('infuser.visit', 'visit-1');
  });
  await visitTab.goto('https://library.test/library');
  await visitTab.locator('#account-trigger').waitFor();
  await visitTab.locator('#account-trigger').click();
  await visitTab.locator('#open-settings').click();
  await visitTab.locator('#user-settings').waitFor({state: 'visible'});
  assert.match(await visitTab.locator('#settings-appearance-scope').innerText(), /Saved for Home Family/);
  assert.match(await visitTab.locator('#settings-profile-summary').innerText(), /Group: Home Family/);
  await visitTab.close();

  // 7. Read-only operator inspection disables appearance pickers and explains why.
  const inspectTab = await context.newPage();
  await inspectTab.goto('https://library.test/profiles');
  await inspectTab.evaluate(() => {
    sessionStorage.setItem('infuser.profile', 'pHome');
    sessionStorage.setItem('infuser.group-inspection', JSON.stringify({group: 'away', expires: Date.now() + 600000}));
  });
  await inspectTab.goto('https://library.test/library');
  await inspectTab.locator('#account-trigger').waitFor();
  await inspectTab.locator('#account-trigger').click();
  await inspectTab.locator('#open-settings').click();
  await inspectTab.locator('#user-settings').waitFor({state: 'visible'});
  assert.equal(await inspectTab.locator('#theme-picker').isDisabled(), true, 'theme picker disabled during inspection');
  assert.match(await inspectTab.locator('#settings-appearance-scope').innerText(), /Read-only inspection/);
  await inspectTab.close();

  assert.deepEqual(errors, []);
  console.log('PASS: Switch A -> B -> A -> B preserves per-profile looks, keeps mic/speaker per device, shows group labels, handles visits & inspection, prunes stale caches, and isolates cross-tab sync.');
} finally {
  await browser.close();
}
