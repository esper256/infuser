// Offline Library polling and interaction soaks: existing Playwright 1.65.1, real web assets, real timers.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {extname} from 'node:path';
import {leakSoak} from './support/leak_soak.mjs';
import {installInteractions, interactionCycle, assertInteractions} from './support/leak_interactions.mjs';
import {installVoiceAccounting} from './support/leak_voice_fixture.mjs';
const args = process.argv.slice(2), options = Object.fromEntries(args.map(arg => arg.replace(/^--/, '').split('=')));
assert(args.every(arg => /^--(?:cycles=\d+|scenario=(?:all|polling|chat|grid|voice))$/.test(arg)) && Object.keys(options).length === args.length,
  'Usage: node tests/leak_soak_journey.mjs [--cycles=2000] [--scenario=all|polling|chat|grid|voice]');
const cycles = Number(options.cycles || 400);
assert(Number.isSafeInteger(cycles) && cycles >= 400 && cycles % 50 === 0, 'Cycles must be at least 400 and a multiple of 50');
const scenarios = !options.scenario || options.scenario === 'all' ? ['polling', 'chat', 'grid', 'voice'] : [options.scenario];
const {chromium} = await import(process.env.INFUSER_PLAYWRIGHT || '../services/peerjs/node_modules/playwright/index.mjs');
const root = new URL('../', import.meta.url);
const origin = 'https://library.test';
const me = {player_id: 'self', name: 'Alex', admin: true, avatars: [], avatar_prompt_seen: true};
const friends = Array.from({length: 40}, (_, i) => ({id: `friend-${i}`, name: `Friend ${i}`}));
const tags = ['Arcade', 'Chill', 'Cooperative', 'Adventure', 'Puzzle', 'Racing', 'Strategy', 'Sandbox'].map((label, i) => ({id: `tag-${i}`, label, count: 40}));
const apps = friends.map((author, i) => ({id: `game-${i}`, title: `Game ${i}`, description: 'A fixture game', author, card_art_url: null, tags: tags.map(t => t.id), players: [], join_targets: [], reactions: [], updated: 1}));
const browser = await chromium.launch({headless: true, executablePath: process.env.INFUSER_CHROMIUM, args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required']});
async function run(scenario) {
let soak, context;
const warmup = {polling: 20, chat: 400, grid: 400, voice: 100}[scenario];
const pollWarmup = scenario === 'polling' ? 0 : 400;
try {
  console.log(`Starting ${scenario} leak soak (${warmup} warmup + ${cycles} cycles)`);
  context = await browser.newContext({viewport: {width: 1200, height: 850}, serviceWorkers: 'block', permissions: ['microphone']});
  const errors = [], unexpected = [];
  // Record only currently live handles, not their history. Native scheduling is untouched.
  const installPolling = voice => {
    const frames = new Set(), raf = window.requestAnimationFrame.bind(window), cancel = window.cancelAnimationFrame.bind(window);
    window.requestAnimationFrame = fn => {let id; id = raf(ts => {frames.delete(id); fn(ts);}); frames.add(id); return id;};
    window.cancelAnimationFrame = id => {frames.delete(id); cancel(id);};
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
    const timers = () => Object.fromEntries(['library', 'client', 'chat', 'voice'].map(name => [name, [...active.values()].filter(t => t.label === name).length]));
    const assertTimers = () => {
      const counts = timers(), expected = {library: 1, client: 1, chat: hidden ? 0 : 1, voice: voice ? 1 : 0};
      for (const name of Object.keys(expected)) if (counts[name] !== expected[name]) throw Error(`${name}: expected ${expected[name]} live timers, found ${counts[name]}`);
    };
    let resolveReady;
    const ready = new Promise(resolve => {resolveReady = resolve;});
    window.__leakSoak = {frames: () => frames.size, ready, resolveReady, hooks, tag, observe, drain, timers, assertTimers,
      async visibility(value) {await drain(); hidden = value; document.dispatchEvent(new Event('visibilitychange')); await drain(); assertTimers();},
      async cycle() {await drain(); await hooks.library(); await hooks.client(); await hooks.chat(); await drain(); assertTimers();},
    };
  };
  // One ordered script: Playwright does not guarantee ordering between init scripts.
  await context.addInitScript({content: `(${installPolling.toString()})(${scenario === 'voice'});${scenario !== 'polling' ? `(${installInteractions.toString()})();` : ''}${scenario === 'voice' ? `(${installVoiceAccounting.toString()})();` : ''}`});
  const fixtureApps = scenario === 'grid' ? apps.map(app => ({...app, card_art_url: '/soak-cover.webp'})) : apps;
  const messages = scenario === 'chat' ? Array.from({length: 60}, (_, i) => ({id: i + 1, author: friends[(Math.floor(i / 5) * 3 + 5) % friends.length], text: `Message ${i + 1}: scroll through our adventures 🎮`, created: 1700000000 + i, can_delete: true})) : [];
  let voiceSelf = null, revision = 0;
  let voiceSettings = {input_gain: 100, output_volume: 100, auto_gain_control: true, noise_suppression: true, peer_volumes: {}};
  const voiceStatus = () => ({configured: true, available: true, epoch: 'fixture', revision, self: voiceSelf, settings: voiceSettings, participants: voiceSelf ? [{id: 'self', name: 'Alex', participant_id: voiceSelf.id}, {id: 'friend-0', name: 'Friend 0', participant_id: 'remote'}] : []});
  const mime = {'.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html', '.json': 'application/json', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.png': 'image/png', '.webp': 'image/webp'};
  function replace(source, before, after) {
    assert.equal(source.split(before).length, 2, `Test hook needs exactly one ${before}`);
    return source.replace(before, after);
  }
  await context.route('**/*', async route => {
    const url = new URL(route.request().url()), path = url.pathname;
    if (url.origin !== origin) {unexpected.push(url.href); return route.abort('blockedbyclient');}
    const responses = {
      '/api/me': me, '/api/library': {apps: fixtureApps, friends, tags}, '/api/chat': {messages, reads: scenario === 'chat' ? [{id: 'self', name: 'Alex', message: 60}] : []},
      '/api/client-state': {game_open: true, version: 'soak'}, '/api/voice': {configured: false},
      '/api/feedback': {available: false}, '/api/presence/input': {ok: true},
      '/api/profile/picture-prompt': {prompt: false}, '/api/telemetry': {ok: true},
    };
    if (path === '/assets/vendor/livekit-2.24.1.js' && scenario === 'voice') return route.fulfill({body: await readFile(new URL('tests/support/leak_voice_fixture.mjs', root)), contentType: 'text/javascript'});
    if (path === '/api/voice' && scenario === 'voice') {
      const method = route.request().method();
      if (method === 'POST') {voiceSelf = {id: route.request().postDataJSON().id, mic_muted: false, deafened: false}; return route.fulfill({json: {...voiceStatus(), id: voiceSelf.id, url: 'wss://never-contact.invalid', token: 'fixture'}});}
      if (method === 'DELETE') voiceSelf = null;
      return route.fulfill({json: voiceStatus()});
    }
    if (path === '/api/voice/settings' && scenario === 'voice') {
      const patch = route.request().postDataJSON(); revision++;
      voiceSettings = {...voiceSettings, ...patch};
      for (const key of ['mic_muted', 'deafened']) if (key in patch && voiceSelf) voiceSelf[key] = patch[key];
      return route.fulfill({json: voiceStatus()});
    }
    if (['/api/voice/ack', '/api/voice/heartbeat', '/api/chat/read'].includes(path)) return route.fulfill({json: {ok: true, message: 60}});
    if (path === '/api/telemetry') return route.fulfill({json: {server_ms: Date.now()}});
    if (path in responses) return route.fulfill({json: responses[path]});
    if (path === '/sw.js' || path === '/favicon.ico') return route.fulfill({status: 404, body: ''});
    const file = path === '/soak-cover.webp' ? 'docs/design-explorations/card-art-guide/firefly-drift.webp' : path.startsWith('/assets/reactions/') && !path.includes('..') ? path.slice(1) : path === '/assets/google-signin.png' ? 'assets/google/sign-in-with-google-pill.png' : path === '/library' ? 'web/index.html' : path.startsWith('/assets/') && !path.includes('..') ? 'web/' + path.slice(8) : null;
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
      body = replace(body, 'setInterval(()=>{if(lease||!document.hidden)void poll();},3000)', "setInterval(window.__leakSoak.tag('voice',()=>{if(lease||!document.hidden)void poll();}),3000)");
      for (const name of scenario === 'voice' ? ['start', 'stop', 'toggleMicTest', 'stopMicTest', 'refreshDevices', 'flushPatch', 'applyAudioState'] : []) {
        body = replace(body, '  const timer=setInterval(', `  ${name}=window.__leakSoak.observe('voice-${name}',${name});\n  const timer=setInterval(`);
      }
    }
    return route.fulfill({body, contentType: mime[extname(file)] || 'application/octet-stream'});
  });
  const page = await context.newPage();
  page.on('pageerror', error => errors.push(error.message));
  const cdp = await context.newCDPSession(page);
  soak = await leakSoak(cdp, {metadata: {scenario, pollWarmup, warmup, cycles}, directory: new URL(`.local/leak-soak/${scenario}/`, root).pathname, settle: () => page.evaluate(interactive => interactive ? window.__leakSoak.settleUI() : window.__leakSoak.drain(), scenario !== 'polling')});
  await page.goto(origin + '/library', {waitUntil: 'load'});
  await page.evaluate(async () => {await window.__leakSoak.ready; await document.fonts.ready; await window.__leakSoak.drain();});
  assert.equal(await page.locator('.game').count(), 40);
  assert.equal(await page.locator('#chat-toggle').getAttribute('aria-expanded'), 'false');
  const cycle = scenario === 'polling' ? () => page.evaluate(() => window.__leakSoak.cycle()) : () => interactionCycle(page, scenario);
  // Real intervals alone exercise these paths too slowly to warm their code
  // before an interaction baseline (e.g. Library runs every 25 seconds).
  for (let i = 0; i < pollWarmup; i++) await page.evaluate(() => window.__leakSoak.cycle());
  for (let i = 0; i < warmup; i++) await cycle();
  await soak.checkpoint('baseline');
  for (let i = 1; i <= cycles; i++) {
    if (scenario === 'polling' && i === 101) await page.evaluate(() => window.__leakSoak.visibility(true));
    if (scenario === 'polling' && i === 151) await page.evaluate(() => window.__leakSoak.visibility(false));
    await cycle();
    if (i % 50 === 0) {
      assert.deepEqual(errors, [], 'No browser errors');
      assert.deepEqual(unexpected, [], 'All traffic stays in the offline fixture');
      if (scenario !== 'polling') await assertInteractions(page, scenario, i + warmup);
      await soak.checkpoint(`cycles-${i}`);
      await page.evaluate(async () => {await window.__leakSoak.drain(); window.__leakSoak.assertTimers();});
    }
  }
  await soak.finish();
  console.log(`PASS: ${scenario}: ${warmup} warmup + ${cycles} cycles; exact resource counts, heap gates and single poll timers.`);
} catch (error) {
  if (soak) {try {await soak.failure(error);} catch (snapshotError) {console.error('Failure snapshot unavailable:', snapshotError.message);}}
  throw error;
} finally {await context?.close();}
}
try {
  const failures = [];
  for (const scenario of scenarios) {
    try {await run(scenario);} catch (error) {failures.push(new Error(`${scenario}: ${error.message}`, {cause: error}));}
  }
  if (failures.length) throw new AggregateError(failures, `Leak soak failed: ${failures.map(error => error.message).join('; ')}`);
} finally {await browser.close();}
