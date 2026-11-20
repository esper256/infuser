/* THROWAWAY. Renders the real Library (web/index.html, clubhouse.css, clubhouse.js)
 * against a mock API and writes Cartridge Club mockups plus a Nightglass pixel diff.
 * Not a product test and not wired to the server.
 *
 *   npm install --prefix /tmp/theme-shots puppeteer-core pngjs
 *   python3 -m pip install pillow
 *   node docs/design-explorations/cartridge-club-showcase/shoot.mjs
 *
 * SHOTS=preview captures one desktop grid pair and its Nightglass diff.
 */
import {createRequire} from 'node:module';
import {createServer} from 'node:http';
import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import path from 'node:path';

const require = createRequire('/tmp/theme-shots/package.json');
const puppeteer = require('puppeteer-core');
const {PNG} = require('pngjs');

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const prototype = path.join(repo, 'docs/design-explorations/cartridge-club-showcase');
const out = process.env.SHOT_DIR || '/tmp/cartridge-shots';
const chrome = process.env.INFUSER_CHROMIUM || '/usr/local/bin/google-chrome';
const NOW = Date.UTC(2026, 10, 20, 15, 0, 0);
const preview = process.env.SHOTS === 'preview';

mkdirSync(out, {recursive: true});
mkdirSync('/tmp/theme-art', {recursive: true});
const crop = spawnSync('python3', ['-c', `
from PIL import Image
from pathlib import Path
atlas = Image.open(${JSON.stringify(path.join(repo, 'docs/design-explorations/nightglass-library/art-atlas.webp'))}).convert('RGB')
w, h = atlas.size
cw, ch = w // 3, h // 3
for i in range(9):
    x, y = (i % 3) * cw, (i // 3) * ch
    atlas.crop((x, y, x + cw, y + ch)).save(f'/tmp/theme-art/{i}.webp', 'WEBP', quality=82)
`], {encoding: 'utf8'});
if (crop.status !== 0) throw new Error(crop.stderr || 'atlas crop failed');

const clubhouse = readFileSync(path.join(repo, 'web/clubhouse.css'), 'utf8');
const needle = 'header, [data-theme="cartridge-club"] header { column-gap: 8px; }';
if (!clubhouse.includes(needle)) throw new Error('shared selector text moved; update the prototype patch');
const clubhousePatched = clubhouse.replace(needle, 'header { column-gap: 8px; }');
const prototypeCss = readFileSync(path.join(prototype, 'theme.css'));

const avatarSource = readFileSync(path.join(repo, 'src/portal/avatars.rs'), 'utf8');
const avatars = [...avatarSource.matchAll(/avatar!\("([a-z0-9-]+)", "([^"]+)"\)/g)].map(([, id, name]) => ({id, name}));
const sec = NOW / 1000;
const author = {id: 'one', name: 'KwamiKwami', avatar: 'loot-raccoon'};
const friend = {id: 'two', name: 'OrbitFox', avatar: 'ember-raven'};
const pal = {id: 'three', name: 'Moss', avatar: 'moss-knight'};
const tags = ['3d', 'action', 'adventure', 'arcade', 'chill', 'co-op', 'exploration'];
function game(fields) {
  return {
    can_manage_visibility: false, can_moderate_reactions: false, can_edit_tags: false,
    description: 'A short flight with friends.', author, updated: sec - 86400,
    url: 'https://library.test/games/' + fields.id, play_modes: ['solo'], players: [],
    join_targets: [], reactions: [], tags: ['chill'], preview: false,
    update_id: fields.id + '-release', update_reactions: {mine: null, counts: {}},
    card_art_url: null, ...fields,
  };
}
const apps = [
  game({id: 'firefly-drift', title: 'Firefly Drift', description: 'A quiet flight through a glowing forest, with room for one friend on the wing.', card_art_url: '/fixture-art/firefly.webp', tags: ['3d', 'arcade', 'chill'], updated: sec - 7200, can_edit_tags: true, players: [{id: 'two', name: 'OrbitFox', avatar: 'ember-raven', activity: 'multiplayer'}, {id: 'guest-moss', name: 'Moss', avatar: 'moss-knight', guest: true, activity: 'title_screen'}], reactions: [{id: 'r1', name: 'OrbitFox', avatar: 'ember-raven', emoji: '🐦‍⬛', text: 'Love the atmosphere.', updated: sec - 4000, mine: false}, {id: 'r2', name: 'KwamiKwami', avatar: 'loot-raccoon', emoji: '⭐', text: '', updated: sec - 5000, mine: true}]}),
  game({id: 'lantern-mystery', title: 'Firefly Drift and the Mystery of Lantern Lake', card_art_url: '/fixture-art/1.webp', tags: ['adventure', 'exploration'], author: friend, updated: sec - 86400}),
  game({id: 'lantern-lake', title: 'Lantern Lake', card_art_url: null, tags: ['chill', 'co-op'], author: pal, updated: sec - 172800}),
  game({id: 'moss-garden', title: 'Moss Garden', card_art_url: '/fixture-art/2.webp', tags: ['co-op'], author: pal, updated: sec - 200000}),
  game({id: 'pizza-rally', title: 'Pizza Planet Rally', card_art_url: '/fixture-art/3.webp', tags: ['action', 'arcade', '3d', 'co-op', 'exploration'], author: friend, updated: sec - 300000}),
  game({id: 'go', title: 'Go', card_art_url: '/fixture-art/4.webp', tags: ['arcade'], author: null, updated: sec - 400000}),
  game({id: 'hidden-shelf', title: 'Hidden Shelf', preview: true, author: null, updated: sec - 500000}),
];
const library = {
  apps,
  friends: [{...author, status: 'active'}, {...friend, status: 'active'}, {...pal, status: 'active'}],
  tags: tags.map(id => ({id, label: id === '3d' ? '3D' : id === 'co-op' ? 'Co-op' : id[0].toUpperCase() + id.slice(1), count: apps.filter(g => g.tags.includes(id)).length})),
  platform_updates: [{id: 'release-125', at: sec - 20, lines: ['Recently lists each healthy Infuser release.'], reactions: {counts: {}, mine: null}}],
  visit: null,
};
const me = {name: author.name, player_id: author.id, admin: true, operator: false, inspecting: false, avatar: author.avatar, avatars, profile_count: 1, avatar_prompt_seen: true};

let mode = 'live';
const types = {'.css': 'text/css', '.js': 'text/javascript', '.webp': 'image/webp', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json', '.webmanifest': 'application/manifest+json', '.html': 'text/html'};
function send(res, status, body, type) {
  res.writeHead(status, {'Content-Type': type, 'Cache-Control': 'no-store'});
  res.end(body);
}
function file(res, filePath) {
  if (!existsSync(filePath)) { send(res, 404, 'missing ' + filePath, 'text/plain'); console.log('missing', filePath); return; }
  const ext = path.extname(filePath);
  send(res, 200, readFileSync(filePath), types[ext] || 'application/octet-stream');
}
const server = createServer((req, res) => {
  const url = new URL(req.url, 'http://library.test');
  const pathname = decodeURIComponent(url.pathname);
  if (pathname.startsWith('/api/')) {
    const data = pathname === '/api/me' ? me
      : pathname === '/api/library' ? library
      : pathname === '/api/chat' ? {messages: []}
      : pathname === '/api/client-state' ? {version: 'review', game_open: false}
      : pathname === '/api/voice' ? {configured: false, available: false, participants: []}
      : pathname === '/api/visits' ? {visits: []}
      : pathname === '/api/screenshots' ? {screenshots: []}
      : pathname === '/api/presence/input' ? {ok: true}
      : {ok: true};
    send(res, 200, JSON.stringify(data), 'application/json');
    return;
  }
  if (pathname === '/assets/themes/cartridge-club.css') {
    const body = mode === 'cartridge' ? prototypeCss : readFileSync(path.join(repo, 'web/themes/cartridge-club.css'));
    send(res, 200, body, 'text/css');
    return;
  }
  if (pathname === '/assets/clubhouse.css') {
    send(res, 200, url.searchParams.has('patched') ? clubhousePatched : clubhouse, 'text/css');
    return;
  }
  if (pathname === '/prototype/theme.css') { send(res, 200, prototypeCss, 'text/css'); return; }
  if (pathname === '/sw.js') {
    send(res, 200, "self.addEventListener('install',e=>self.skipWaiting());self.addEventListener('activate',e=>self.clients.claim());", 'text/javascript');
    return;
  }
  if (pathname === '/assets/app-192.png' || pathname === '/assets/app-512.png') {
    file(res, path.join(repo, 'assets/branding/infuser', pathname.endsWith('512.png') ? 'icon-512.png' : 'icon-192.png'));
    return;
  }
  if (pathname.startsWith('/assets/themes/cartridge-club/')) {
    file(res, path.join(prototype, 'assets', pathname.slice('/assets/themes/cartridge-club/'.length)));
    return;
  }
  if (pathname.startsWith('/fixture-art/')) {
    const name = pathname.slice('/fixture-art/'.length);
    if (name === 'firefly.webp') file(res, path.join(repo, 'docs/design-explorations/card-art-guide/firefly-drift.webp'));
    else file(res, path.join('/tmp/theme-art', name));
    return;
  }
  const avatar = pathname.match(/^\/assets\/avatars\/([^/]+)\/(\d+)\.webp$/);
  if (avatar) { file(res, path.join(repo, 'assets/avatars', `${avatar[1]}-${avatar[2]}.webp`)); return; }
  if (pathname.startsWith('/assets/reactions/')) { file(res, path.join(repo, 'assets/reactions', pathname.slice('/assets/reactions/'.length))); return; }
  if (pathname === '/assets/card-generating.webp') { file(res, path.join(repo, 'assets/card-fallback/generating.webp')); return; }
  if (pathname === '/assets/google-signin.png') { file(res, path.join(repo, 'assets/google/sign-in-with-google-pill.png')); return; }
  if (pathname === '/assets/apple-touch-icon.png') { file(res, path.join(repo, 'assets/branding/infuser/apple-touch-icon.png')); return; }
  if (pathname === '/manifest.webmanifest') { file(res, path.join(repo, 'web/manifest.webmanifest')); return; }
  if (pathname.startsWith('/assets/')) { file(res, path.join(repo, 'web', pathname.slice('/assets/'.length))); return; }
  file(res, path.join(repo, 'web/index.html'));
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const port = server.address().port;

const browser = await puppeteer.launch({
  executablePath: chrome,
  headless: 'new',
  protocolTimeout: 120000,
  args: ['--no-sandbox', '--disable-dev-shm-usage', '--force-color-profile=srgb', '--font-render-hinting=none', '--disable-lcd-text'],
});
const page = await browser.newPage();
const errors = [];
page.on('pageerror', error => errors.push(error.message));
page.on('console', message => { if (process.env.DEBUG) console.log('console', message.type(), message.text()); });
await page.evaluateOnNewDocument(now => {
  try { localStorage.removeItem('infuser.theme'); } catch { /* boot Nightglass */ }
  Date.now = () => now;
  const original = window.setInterval;
  window.setInterval = (fn, ms, ...rest) => (ms >= 1000 ? 0 : original(fn, ms, ...rest));
}, NOW);
await page.emulateMediaFeatures([{name: 'prefers-reduced-motion', value: 'reduce'}]);

async function settle() {
  await page.waitForSelector('#visible-games .game');
  await page.waitForSelector('#chat-toggle');
  await page.waitForSelector('#account-trigger');
  await page.waitForFunction(() => {
    const button = [...document.querySelectorAll('button')].find(node => node.textContent.startsWith('Group Chat notifications'));
    return button && !button.disabled;
  }, {timeout: 8000}).catch(() => {});
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
async function shot(name) {
  const filePath = path.join(out, name + '.png');
  await page.screenshot({path: filePath, type: 'png'});
  return filePath;
}
async function openScene(scene) {
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
}
async function closeScene(scene) {
  if (scene === 'detail') {
    await page.click('#close-game-detail');
    await page.waitForFunction(() => !document.querySelector('#game-detail').open);
  } else if (scene === 'recent') await page.click('#close-recent');
  else if (scene === 'profile') await page.click('#account-trigger');
}
async function load(themeMode, view) {
  mode = themeMode === 'cartridge' ? 'cartridge' : 'live';
  await page.setViewport({width: view.width, height: view.height, deviceScaleFactor: 1});
  await page.goto(`http://127.0.0.1:${port}/library`, {waitUntil: 'domcontentloaded', timeout: 60000});
  await page.evaluate(theme => window.InfuserThemes.select(theme), themeMode === 'cartridge' ? 'cartridge-club' : 'nightglass');
  await settle();
}
async function replaceClubhouse(href) {
  await page.evaluate(href => new Promise(resolve => {
    const current = document.querySelector('link[href*="clubhouse.css"]');
    const next = current.cloneNode();
    next.href = href;
    next.onload = () => { current.remove(); resolve(); };
    current.after(next);
  }), href);
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}
async function injectAfter() {
  await page.evaluate(() => new Promise(resolve => {
    let pending = 2;
    const done = () => { if (--pending === 0) resolve(); };
    const current = document.querySelector('link[href*="clubhouse.css"]');
    const next = current.cloneNode();
    next.href = '/assets/clubhouse.css?patched=1';
    next.onload = () => { current.remove(); done(); };
    current.after(next);
    const theme = document.createElement('link');
    theme.id = 'prototype-theme';
    theme.rel = 'stylesheet';
    theme.href = '/prototype/theme.css';
    theme.onload = done;
    document.head.append(theme);
  }));
  await page.evaluate(async () => {
    await document.fonts.ready;
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
}
async function revertAfter() {
  await page.evaluate(() => document.querySelector('#prototype-theme')?.remove());
  await replaceClubhouse('/assets/clubhouse.css?restored=1');
}
async function capture(themeMode, view) {
  await load(themeMode, view);
  const prefix = `${themeMode}-${view.id}`;
  const shots = {};
  for (const scene of ['grid', 'detail', 'recent', 'profile']) {
    await openScene(scene);
    shots[scene] = await shot(`${prefix}-${scene}`);
    await closeScene(scene);
  }
  return shots;
}

const views = preview
  ? [{id: 'desktop', width: 1440, height: 1180}]
  : [{id: 'desktop', width: 1440, height: 1180}, {id: 'tablet', width: 768, height: 1100}, {id: 'phone', width: 390, height: 980}, {id: 'phone320', width: 320, height: 780}];
const mockViews = views;
const scenes = ['grid', 'detail', 'recent', 'profile'];
if (process.env.STABILITY) {
  await load('nightglass', views[0]);
  await replaceClubhouse('/assets/clubhouse.css?warm=1');
  const first = await shot('stability-a');
  if (process.env.STABILITY === 'reload') await replaceClubhouse('/assets/clubhouse.css?warm=2');
  else if (process.env.STABILITY === 'patch') await replaceClubhouse('/assets/clubhouse.css?patched=1');
  else if (process.env.STABILITY === 'theme') {
    await page.evaluate(() => new Promise(resolve => {
      const theme = document.createElement('link');
      theme.rel = 'stylesheet'; theme.href = '/prototype/theme.css'; theme.onload = resolve;
      document.head.append(theme);
    }));
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  } else if (process.env.STABILITY === 'settled') {
    await page.evaluate(() => new Promise(resolve => {
      const theme = document.createElement('link');
      theme.rel = 'stylesheet'; theme.href = '/prototype/theme.css'; theme.onload = resolve;
      document.head.append(theme);
    }));
    await page.evaluate(() => document.fonts.ready);
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    const mid = await shot('stability-mid');
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    const third = await shot('stability-c');
    const M = PNG.sync.read(readFileSync(mid));
    const C = PNG.sync.read(readFileSync(third));
    let settled = 0;
    for (let i = 0; i < M.data.length; i += 4) if (M.data[i] !== C.data[i] || M.data[i + 1] !== C.data[i + 1] || M.data[i + 2] !== C.data[i + 2] || M.data[i + 3] !== C.data[i + 3]) settled++;
    console.log('settled-after-fonts', settled);
  } else await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const second = await shot('stability-b');
  const A = PNG.sync.read(readFileSync(first));
  const B = PNG.sync.read(readFileSync(second));
  let n = 0;
  for (let i = 0; i < A.data.length; i += 4) if (A.data[i] !== B.data[i] || A.data[i + 1] !== B.data[i + 1] || A.data[i + 2] !== B.data[i + 2] || A.data[i + 3] !== B.data[i + 3]) n++;
  if (process.env.STABILITY === 'theme') {
    console.log(await page.evaluate(() => ({
      theme: document.documentElement.dataset.theme,
      body: getComputedStyle(document.body).backgroundImage.slice(0, 180),
      font: getComputedStyle(document.body).fontFamily,
      header: getComputedStyle(document.querySelector('header')).borderRadius,
      h2: getComputedStyle(document.querySelector('.game h2')).fontFamily,
    })));
  }
  console.log('notifications', await page.evaluate(() => {
    const button = [...document.querySelectorAll('button')].find(node => node.textContent.startsWith('Group Chat'));
    const hint = document.querySelector('.notification-hint');
    return {text: button?.textContent, disabled: button?.disabled, hint: hint?.textContent};
  }));
  console.log('stability', process.env.STABILITY, n);
  await browser.close(); server.close(); process.exit(0);
}
const captured = {};
for (const view of views) {
  console.log('diff', view.id);
  await load('nightglass', view);
  await replaceClubhouse('/assets/clubhouse.css?warm=1');
  captured[`before-${view.id}`] = {};
  captured[`after-${view.id}`] = {};
  for (const scene of scenes) {
    await openScene(scene);
    captured[`before-${view.id}`][scene] = await shot(`before-${view.id}-${scene}`);
    // The shared-file edit only. Prototype rules are measured separately: they do
    // not change computed Nightglass styles, and a second frame after font load is 0.
    await replaceClubhouse('/assets/clubhouse.css?patched=1');
    captured[`after-${view.id}`][scene] = await shot(`after-${view.id}-${scene}`);
    await replaceClubhouse('/assets/clubhouse.css?restored=1');
    await closeScene(scene);
  }
}
for (const view of mockViews) {
  console.log('mock', view.id);
  captured[`cartridge-${view.id}`] = await capture('cartridge', view);
}
await browser.close();
server.close();

function pixels(filePath) {
  return PNG.sync.read(readFileSync(filePath));
}
const report = [];
let failed = 0;
for (const view of views) {
  for (const scene of scenes) {
    const before = pixels(captured[`before-${view.id}`][scene]);
    const after = pixels(captured[`after-${view.id}`][scene]);
    let count = null;
    let note = '';
    if (before.width !== after.width || before.height !== after.height) {
      count = -1;
      note = `size ${before.width}x${before.height} vs ${after.width}x${after.height}`;
      failed++;
    } else {
      count = 0;
      const diff = new PNG({width: before.width, height: before.height});
      for (let i = 0; i < before.data.length; i += 4) {
        const changed = before.data[i] !== after.data[i] || before.data[i + 1] !== after.data[i + 1] || before.data[i + 2] !== after.data[i + 2] || before.data[i + 3] !== after.data[i + 3];
        if (changed) {
          count++;
          diff.data[i] = 255; diff.data[i + 1] = 0; diff.data[i + 2] = 0; diff.data[i + 3] = 255;
        } else {
          diff.data[i + 3] = 255;
        }
      }
      writeFileSync(path.join(out, `diff-${view.id}-${scene}.png`), PNG.sync.write(diff));
      if (count) failed++;
    }
    report.push({view: view.id, scene, width: view.width, pixels: count, note});
    console.log(`${view.id} ${scene}: ${count} ${note}`);
  }
}
writeFileSync(path.join(out, 'diff-report.json'), JSON.stringify({failed, errors, report}, null, 2));
if (errors.length) console.log('page errors', errors);

const stitch = spawnSync('python3', ['-c', String.raw`
import json
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont
out = Path(${JSON.stringify(out)})
report = json.loads((out / "diff-report.json").read_text())
font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 22)
small = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 16)
def label(text, width, fill):
    img = Image.new("RGB", (width, 36), fill)
    draw = ImageDraw.Draw(img)
    draw.text((12, 6), text, fill="white", font=font)
    return img
def pair(left, right, left_name, right_name, caption, dest):
    a, b = Image.open(left).convert("RGB"), Image.open(right).convert("RGB")
    gap = 8
    width = a.width + b.width + gap
    canvas = Image.new("RGB", (width, 36 + max(a.height, b.height) + 28), (20, 16, 28))
    canvas.paste(label(left_name, a.width, (18, 42, 48)), (0, 0))
    canvas.paste(label(right_name, b.width, (90, 40, 120)), (a.width + gap, 0))
    canvas.paste(a, (0, 36))
    canvas.paste(b, (a.width + gap, 36))
    ImageDraw.Draw(canvas).text((12, canvas.height - 24), caption, fill="white", font=small)
    canvas.save(dest, optimize=True)
scenes = ["grid", "detail", "recent", "profile"]
names = {"grid": "Library grid", "detail": "Game detail", "recent": "Recently", "profile": "Profile menu"}
sizes = {"desktop": "1440 desktop", "tablet": "768 tablet", "phone": "390 phone", "phone320": "320 phone"}
for view, size in sizes.items():
    for scene in scenes:
        left = out / f"before-{view}-{scene}.png"
        right = out / f"cartridge-{view}-{scene}.png"
        if left.exists() and right.exists():
            pair(left, right, "Nightglass", "Cartridge Club", f"{names[scene]} · {size}", out / f"side-{view}-{scene}.png")
# Pixel-diff proof: desktop grid before/after, plus every scene count.
before = Image.open(out / "before-desktop-grid.png").convert("RGB")
after = Image.open(out / "after-desktop-grid.png").convert("RGB")
diff = Image.open(out / "diff-desktop-grid.png").convert("RGB")
scale = 520 / before.width
def fit(img):
    return img.resize((520, round(img.height * scale)), Image.Resampling.BOX)
b, a, d = fit(before), fit(after), fit(diff)
head = 64
row = b.height
table_h = 28 + 22 * len(report["report"])
canvas = Image.new("RGB", (520 * 3 + 16, head + row + table_h + 16), (12, 10, 16))
draw = ImageDraw.Draw(canvas)
draw.text((12, 8), "Nightglass before  |  Nightglass after the shared selector edit  |  differing pixels in red", fill="white", font=small)
draw.text((12, 32), "Fail the check on any difference. Reduced motion, same fixture, same browser.", fill=(220, 210, 230), font=small)
canvas.paste(b, (0, head)); canvas.paste(a, (528, head)); canvas.paste(d, (1056, head))
y = head + row + 8
total = 0
for item in report["report"]:
    total += max(item["pixels"], 0)
    draw.text((12, y), f"{item['view']:8} {item['scene']:8} {item['pixels']} pixels {item['note']}", fill=(180, 255, 190) if item["pixels"] == 0 else (255, 120, 120), font=small)
    y += 22
draw.text((700, y - 4), f"total differing pixels: {total}", fill="white", font=font)
canvas.save(out / "nightglass-pixel-diff.png", optimize=True)
print("stitched", total)
`], {encoding: 'utf8'});
if (stitch.status !== 0) {
  console.error(stitch.stderr);
  process.exit(1);
}
console.log(stitch.stdout);
if (failed || errors.length) process.exit(1);
