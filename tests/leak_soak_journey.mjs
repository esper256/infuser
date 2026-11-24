// Offline Library polling soak: existing Playwright 1.65.1, real web assets, real timers.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {extname} from 'node:path';
import {leakSoak} from './support/leak_soak.mjs';
const {chromium} = await import(process.env.INFUSER_PLAYWRIGHT || '../services/peerjs/node_modules/playwright/index.mjs');
const root = new URL('../', import.meta.url);
const origin = 'https://library.test';
const me = {player_id: 'self', name: 'Alex', admin: true, avatars: [], avatar_prompt_seen: true};
const friends = Array.from({length: 40}, (_, i) => ({id: `friend-${i}`, name: `Friend ${i}`}));
const tags = ['Arcade', 'Chill', 'Cooperative', 'Adventure', 'Puzzle', 'Racing', 'Strategy', 'Sandbox'].map((label, i) => ({id: `tag-${i}`, label, count: 40}));
const apps = friends.map((author, i) => ({id: `game-${i}`, title: `Game ${i}`, description: 'A fixture game', author, card_art_url: null, tags: tags.map(t => t.id), players: [], join_targets: [], reactions: [], updated: 1}));
const browser = await chromium.launch({headless: true, executablePath: process.env.INFUSER_CHROMIUM});
let soak;
try {
  const context = await browser.newContext({viewport: {width: 1200, height: 850}, serviceWorkers: 'block'});
  const errors = [], unexpected = [];
  // Record only currently live handles, not their history. Native scheduling is untouched.
  await context.addInitScript(() => {
    const active = new Map(), labels = new WeakMap(), pending = new Set();
    const interval = window.setInterval.bind(window), timeout = window.setTimeout.bind(window);
    const clearInterval = window.clearInterval.bind(window), clearTimeout = window.clearTimeout.bind(window);
    const register = (kind, native, callback, delay, args) => {
      let id;
      id = native(() => {if (kind === 'timeout') active.delete(id); return callback(...args);}, delay);
      active.set(id, {kind, label: labels.get(callback)});
      return id;
    };
    window.setInterval = (fn, delay, ...args) => register('interval', interval, fn, delay, args);
    window.setTimeout = (fn, delay, ...args) => register('timeout', timeout, fn, delay, args);
    window.clearInterval = id => {active.delete(id); clearInterval(id);};
    window.clearTimeout = id => {active.delete(id); clearTimeout(id);};
    let hidden = false;
    Object.defineProperty(document, 'hidden', {get: () => hidden, configurable: true});
    Object.defineProperty(document, 'visibilityState', {get: () => hidden ? 'hidden' : 'visible', configurable: true});
    const hooks = {};
    const tag = (name, fn) => {labels.set(fn, name); return fn;};
    const observe = (name, fn) => {
      const wrapped = tag(name, function(...args) {
        const task = fn(...args);
        if (!task || typeof task.then !== 'function') return task;
        const completed = task.finally(() => pending.delete(completed));
        pending.add(completed);
        return completed;
      });
      hooks[name] = wrapped;
      return wrapped;
    };
    const drain = async () => {
      // Await work already in flight, without retrying a poll or advancing time.
      while (pending.size) await Promise.all([...pending]);
    };
    const timers = () => Object.fromEntries(['library', 'client', 'chat'].map(name => [name, [...active.values()].filter(t => t.label === name).length]));
    const assertTimers = () => {
      const counts = timers(), expected = {library: 1, client: 1, chat: hidden ? 0 : 1};
      for (const name of Object.keys(expected)) if (counts[name] !== expected[name]) throw Error(`${name}: expected ${expected[name]} live timers, found ${counts[name]}`);
    };
    let resolveReady;
    const ready = new Promise(resolve => {resolveReady = resolve;});
    window.__leakSoak = {ready, resolveReady, hooks, tag, observe, drain, timers, assertTimers,
      async visibility(value) {await drain(); hidden = value; document.dispatchEvent(new Event('visibilitychange')); await drain(); assertTimers();},
      async cycle() {await drain(); await hooks.library(); await hooks.client(); await hooks.chat(); await drain(); assertTimers();},
    };
  });
  const mime = {'.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html', '.json': 'application/json', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.png': 'image/png', '.webp': 'image/webp'};
  function replace(source, before, after) {
    assert.equal(source.split(before).length, 2, `Test hook needs exactly one ${before}`);
    return source.replace(before, after);
  }
  await context.route('**/*', async route => {
    const url = new URL(route.request().url()), path = url.pathname;
    if (url.origin !== origin) {unexpected.push(url.href); return route.abort('blockedbyclient');}
    const responses = {
      '/api/me': me, '/api/library': {apps, friends, tags}, '/api/chat': {messages: [], reads: []},
      '/api/client-state': {game_open: true, version: 'soak'}, '/api/voice': {configured: false},
      '/api/feedback': {available: false}, '/api/presence/input': {ok: true},
      '/api/profile/picture-prompt': {prompt: false}, '/api/telemetry': {ok: true},
    };
    if (path === '/api/telemetry') return route.fulfill({json: {server_ms: Date.now()}});
    if (path in responses) return route.fulfill({json: responses[path]});
    if (path === '/sw.js' || path === '/favicon.ico') return route.fulfill({status: 404, body: ''});
    const file = path.startsWith('/assets/reactions/') && !path.includes('..') ? path.slice(1) : path === '/assets/google-signin.png' ? 'assets/google/sign-in-with-google-pill.png' : path === '/library' ? 'web/index.html' : path.startsWith('/assets/') && !path.includes('..') ? 'web/' + path.slice(8) : null;
    if (!file) {unexpected.push(path); return route.fulfill({status: 404, body: ''});}
    let body;
    try {body = await readFile(new URL(file, root));}
    catch {unexpected.push(path); return route.fulfill({status: 404, body: ''});}
    if (file === 'web/clubhouse.js') {
      body = replace(body.toString(), 'try {\n  me=await api', "refresh=window.__leakSoak.observe('library',refresh);\ntry {\n  me=await api");
      body = replace(body, 'setInterval(()=>{if(!document.hidden)refresh();},25000);', "setInterval(window.__leakSoak.tag('library',()=>{if(!document.hidden)refresh();}),25000);");
      body += '\nawait window.__leakSoak.drain();window.__leakSoak.resolveReady();';
    } else if (file === 'web/client-updates.js') {
      body = replace(body.toString(), '  win.setInterval(check,30000);', "  check=window.__leakSoak.observe('client',check);\n  win.setInterval(check,30000);");
    } else if (file === 'web/chat.js') {
      body = replace(body.toString(), "  document.addEventListener('visibilitychange',()=>{if(document.hidden)clearTimeout(timer);else poll();});", "  poll=window.__leakSoak.observe('chat',poll);\n  document.addEventListener('visibilitychange',()=>{if(document.hidden)clearTimeout(timer);else poll();});");
    } else if (file === 'web/voice.js') {
      body = replace(body.toString(), '  const timer=setInterval(', "  poll=window.__leakSoak.observe('voice',poll);\n  const timer=setInterval(");
    }
    return route.fulfill({body, contentType: mime[extname(file)] || 'application/octet-stream'});
  });
  const page = await context.newPage();
  page.on('pageerror', error => errors.push(error.message));
  const cdp = await context.newCDPSession(page);
  soak = await leakSoak(cdp, {directory: new URL('.local/leak-soak/', root).pathname, settle: () => page.evaluate(() => window.__leakSoak.drain())});
  await page.goto(origin + '/library', {waitUntil: 'load'});
  await page.evaluate(async () => {await window.__leakSoak.ready; await document.fonts.ready; await window.__leakSoak.drain();});
  assert.equal(await page.locator('.game').count(), 40);
  assert.equal(await page.locator('#chat-toggle').getAttribute('aria-expanded'), 'false');
  const cycle = () => page.evaluate(() => window.__leakSoak.cycle());
  for (let i = 0; i < 20; i++) await cycle();
  await soak.checkpoint('baseline');
  for (let i = 1; i <= 200; i++) {
    if (i === 101) await page.evaluate(() => window.__leakSoak.visibility(true));
    if (i === 151) await page.evaluate(() => window.__leakSoak.visibility(false));
    await cycle();
    if (i % 50 === 0) {
      assert.deepEqual(errors, [], 'No browser errors');
      assert.deepEqual(unexpected, [], 'All traffic stays in the offline fixture');
      await soak.checkpoint(`cycles-${i}`);
      await page.evaluate(() => window.__leakSoak.assertTimers());
    }
  }
  await soak.finish();
  console.log('PASS: 20 warmup + 200 Library/client-state/closed-chat cycles; hidden pause/resume; exact resource counts and single poll timers.');
} catch (error) {
  if (soak) await soak.failure(error);
  throw error;
} finally {await browser.close();}
