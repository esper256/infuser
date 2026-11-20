// Nightglass must not change when Cartridge Club's stylesheet is linked.
// Fails on any pixel difference. No retries and no tolerance.
//
// Two checks, same Chrome, same fixture, prefers-reduced-motion:
// 1. Two settled frames of Nightglass are identical.
// 2. Turning off Cartridge Club's rules, while keeping its @font-face
//    rules registered, does not change a single pixel. Font registration
//    itself repaints glyphs once; that repaint happens before either frame.
import {createServer} from 'node:http';
import {existsSync, readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const moduleRoot = process.env.NIGHTGLASS_PIXELS_MODULES || '/tmp/nightglass-pixels/node_modules';
function load(name) {
  const pkg = path.join(moduleRoot, name, 'package.json');
  if (!existsSync(pkg)) return null;
  return createRequire(pkg)(name);
}
const puppeteer = load('puppeteer') || load('puppeteer-core');
if (!puppeteer) throw new Error(`Install puppeteer in ${moduleRoot}`);
const {PNG: Png} = load('pngjs');
if (!Png) throw new Error(`Install pngjs in ${moduleRoot}`);

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const NOW = Date.UTC(2026, 10, 20, 15, 0, 0);
const sec = NOW / 1000;
const author = {id: 'one', name: 'KwamiKwami', avatar: 'loot-raccoon'};
const friend = {id: 'two', name: 'OrbitFox', avatar: 'ember-raven'};
const pal = {id: 'three', name: 'Moss', avatar: 'moss-knight'};
const avatarSource = readFileSync(path.join(repo, 'src/portal/avatars.rs'), 'utf8');
const avatars = [...avatarSource.matchAll(/avatar!\("([a-z0-9-]+)", "([^"]+)"\)/g)].map(([, id, name]) => ({id, name}));
const tags = ['3d', 'action', 'adventure', 'arcade', 'chill', 'co-op', 'exploration'];
function game(fields) {
  return {
    can_manage_visibility: false, can_moderate_reactions: false, can_edit_tags: false,
    description: 'A short flight with friends.', author, updated: sec - 86400,
    play_modes: ['solo'], players: [], join_targets: [], reactions: [], tags: ['chill'],
    preview: false, update_id: fields.id + '-release', update_reactions: {mine: null, counts: {}},
    card_art_url: null, url: 'https://library.test/games/' + fields.id, ...fields,
  };
}
const apps = [
  game({id: 'firefly-drift', title: 'Firefly Drift', description: 'A quiet flight through a glowing forest, with room for one friend on the wing.', card_art_url: '/assets/avatars/loot-raccoon/256.webp', tags: ['3d', 'arcade', 'chill'], updated: sec - 7200, can_edit_tags: true, players: [{id: 'two', name: 'OrbitFox', avatar: 'ember-raven', activity: 'multiplayer'}, {id: 'guest-moss', name: 'Moss', avatar: 'moss-knight', guest: true, activity: 'title_screen'}], reactions: [{id: 'r1', name: 'OrbitFox', avatar: 'ember-raven', emoji: '🐦‍⬛', text: 'Love the atmosphere.', updated: sec - 4000, mine: false}]}),
  game({id: 'lantern-mystery', title: 'Firefly Drift and the Mystery of Lantern Lake', card_art_url: '/assets/avatars/orbit-racer/256.webp', tags: ['adventure', 'exploration'], author: friend, updated: sec - 86400}),
  game({id: 'lantern-lake', title: 'Lantern Lake', card_art_url: null, tags: ['chill', 'co-op'], author: pal, updated: sec - 172800}),
];
const library = {
  apps,
  friends: [{...author, status: 'active'}, {...friend, status: 'active'}, {...pal, status: 'active'}],
  tags: tags.map(id => ({id, label: id === '3d' ? '3D' : id === 'co-op' ? 'Co-op' : id[0].toUpperCase() + id.slice(1), count: apps.filter(item => item.tags.includes(id)).length})),
  platform_updates: [{id: 'release-125', at: sec - 20, lines: ['Recently lists each healthy Infuser release.'], reactions: {counts: {}, mine: null}}],
  visit: null,
};
const me = {name: author.name, player_id: author.id, admin: true, operator: false, inspecting: false, avatar: author.avatar, avatars, profile_count: 1, avatar_prompt_seen: true};
const types = {'.css': 'text/css', '.js': 'text/javascript', '.webp': 'image/webp', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.txt': 'text/plain', '.md': 'text/plain', '.json': 'application/json', '.webmanifest': 'application/manifest+json', '.html': 'text/html'};

function send(res, status, body, type) {
  res.writeHead(status, {'Content-Type': type, 'Cache-Control': 'no-store'});
  res.end(body);
}
function file(res, filePath) {
  if (!existsSync(filePath)) { send(res, 404, 'missing', 'text/plain'); return; }
  send(res, 200, readFileSync(filePath), types[path.extname(filePath)] || 'application/octet-stream');
}
const server = createServer((req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const pathname = decodeURIComponent(url.pathname);
  if (pathname.startsWith('/api/')) {
    const data = pathname === '/api/me' ? me
      : pathname === '/api/library' ? library
      : pathname === '/api/chat' ? {messages: []}
      : pathname === '/api/client-state' ? {version: 'review', game_open: false}
      : pathname === '/api/voice' ? {configured: false, available: false, participants: []}
      : pathname === '/api/visits' ? {visits: []}
      : pathname === '/api/screenshots' ? {screenshots: []}
      : {ok: true};
    send(res, 200, JSON.stringify(data), 'application/json');
    return;
  }
  if (pathname.startsWith('/assets/themes/')) {
    const themePath = pathname.slice('/assets/themes/'.length);
    file(res, path.join(repo, 'web/themes', themePath));
    return;
  }
  const avatar = pathname.match(/^\/assets\/avatars\/([^/]+)\/(\d+)\.webp$/);
  if (avatar) { file(res, path.join(repo, 'assets/avatars', `${avatar[1]}-${avatar[2]}.webp`)); return; }
  if (pathname.startsWith('/assets/reactions/')) { file(res, path.join(repo, 'assets/reactions', pathname.slice('/assets/reactions/'.length))); return; }
  if (pathname === '/assets/card-generating.webp') { file(res, path.join(repo, 'assets/card-fallback/generating.webp')); return; }
  if (pathname === '/assets/apple-touch-icon.png') { file(res, path.join(repo, 'assets/branding/infuser/apple-touch-icon.png')); return; }
  if (pathname === '/assets/app-192.png') { file(res, path.join(repo, 'assets/branding/infuser/icon-192.png')); return; }
  if (pathname === '/manifest.webmanifest') { file(res, path.join(repo, 'web/manifest.webmanifest')); return; }
  if (pathname === '/sw.js') { send(res, 200, "self.addEventListener('install',e=>self.skipWaiting());self.addEventListener('activate',e=>self.clients.claim());", 'text/javascript'); return; }
  if (pathname.startsWith('/assets/')) { file(res, path.join(repo, 'web', pathname.slice('/assets/'.length))); return; }
  file(res, path.join(repo, 'web/index.html'));
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const port = server.address().port;
const executablePath = process.env.INFUSER_CHROMIUM || (typeof puppeteer.executablePath === 'function' ? puppeteer.executablePath() : '/usr/local/bin/google-chrome');
const browser = await puppeteer.launch({
  executablePath,
  headless: true,
  protocolTimeout: 120000,
  args: ['--no-sandbox', '--disable-dev-shm-usage', '--force-color-profile=srgb', '--font-render-hinting=none', '--disable-lcd-text'],
});
const page = await browser.newPage();
const errors = [];
page.on('pageerror', error => errors.push(error.message));
await page.evaluateOnNewDocument(now => {
  try { localStorage.removeItem('infuser.theme'); } catch { /* Nightglass. */ }
  Date.now = () => now;
  const original = window.setInterval;
  window.setInterval = (fn, ms, ...rest) => (ms >= 1000 ? 0 : original(fn, ms, ...rest));
}, NOW);
await page.emulateMediaFeatures([{name: 'prefers-reduced-motion', value: 'reduce'}]);

async function settle() {
  await page.waitForSelector('#visible-games .game');
  await page.waitForSelector('#account-trigger');
  await page.waitForFunction(() => {
    const button = [...document.querySelectorAll('button')].find(node => node.textContent.startsWith('Group Chat notifications'));
    return button && !button.disabled;
  });
  await page.evaluate(async () => {
    window.scrollTo(0, 0);
    for (const scroller of document.querySelectorAll('.detail-info, #activity, .account-menu')) scroller.scrollTop = 0;
    await Promise.race([document.fonts.ready, new Promise(resolve => setTimeout(resolve, 4000))]);
    await Promise.race([
      Promise.all([...document.images].map(img => new Promise(resolve => {
        const done = () => resolve();
        img.loading = 'eager';
        img.addEventListener('load', done, {once: true});
        img.addEventListener('error', done, {once: true});
        if (img.complete) done();
      }))),
      new Promise(resolve => setTimeout(resolve, 2500)),
    ]);
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
}
async function pixels() {
  return Png.sync.read(await page.screenshot({type: 'png'}));
}
function difference(left, right) {
  if (left.width !== right.width || left.height !== right.height) return -1;
  let count = 0;
  for (let i = 0; i < left.data.length; i += 4) {
    if (left.data[i] !== right.data[i] || left.data[i + 1] !== right.data[i + 1] || left.data[i + 2] !== right.data[i + 2] || left.data[i + 3] !== right.data[i + 3]) count++;
  }
  return count;
}
async function rules(enabled) {
  await page.evaluate(enabled => {
    const link = document.querySelector('link[href="/assets/themes/cartridge-club.css"]');
    if (!document.getElementById('theme-font-hold')) {
      const sheet = [...document.styleSheets].find(item => item.href && item.href.includes('/assets/themes/cartridge-club.css'));
      const faces = [...sheet.cssRules].filter(rule => rule.type === CSSRule.FONT_FACE_RULE).map(rule => rule.cssText);
      if (!faces.length) throw new Error('Cartridge Club stylesheet has no @font-face rules');
      const hold = document.createElement('style');
      hold.id = 'theme-font-hold';
      hold.textContent = faces.join('\n');
      document.head.append(hold);
    }
    link.disabled = !enabled;
  }, enabled);
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

const views = [
  {id: 'desktop', width: 1440, height: 1180},
  {id: 'tablet', width: 768, height: 1100},
  {id: 'phone', width: 390, height: 980},
  {id: 'phone320', width: 320, height: 780},
];
const scenes = ['grid', 'detail', 'recent', 'profile'];
const failures = [];
try {
  for (const view of views) {
    await page.setViewport({width: view.width, height: view.height, deviceScaleFactor: 1});
    await page.goto(`http://127.0.0.1:${port}/library`, {waitUntil: 'domcontentloaded', timeout: 60000});
    await page.evaluate(() => window.InfuserThemes.select('nightglass'));
    await settle();
    // The first time the candidate sheet is disabled, Chrome repaints glyphs
    // while @font-face moves to the hold sheet. Do that before any comparison.
    await rules(false);
    await settle();
    await rules(true);
    await settle();
    for (const scene of scenes) {
      if (scene === 'detail') {
        await page.click('#visible-games [data-app="firefly-drift"] .game-open');
        await page.waitForSelector('#game-detail[open]');
      } else if (scene === 'recent') {
        await page.click('#recent-toggle');
        await page.waitForSelector('#recent-menu[open] .feed-row');
      } else if (scene === 'profile') {
        await page.click('#account-trigger');
        await page.waitForSelector('.profile-menu[open] .account-menu');
      }
      await settle();
      const first = await pixels();
      await settle();
      const second = await pixels();
      const stable = difference(first, second);
      await rules(false);
      await settle();
      const withoutRules = await pixels();
      const leaked = difference(second, withoutRules);
      await rules(true);
      console.log(`${view.id} ${scene}: stability ${stable}, rules ${leaked}`);
      if (stable !== 0) failures.push(`${view.id} ${scene} moved ${stable} pixels with no theme change`);
      if (leaked !== 0) failures.push(`${view.id} ${scene} changed ${leaked} pixels when Cartridge Club rules were removed`);
      if (scene === 'detail') {
        await page.click('#close-game-detail');
        await page.waitForFunction(() => !document.querySelector('#game-detail').open);
      } else if (scene === 'recent') await page.click('#close-recent');
      else if (scene === 'profile') await page.click('#account-trigger');
    }
  }
} finally {
  await browser.close();
  server.close();
}
if (errors.length) failures.push(`page errors: ${errors.join('; ')}`);
if (failures.length) {
  console.error(failures.join('\n'));
  process.exit(1);
}
console.log('Nightglass pixel diff: 0');
