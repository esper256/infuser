import assert from 'node:assert/strict';

// Await rendering work and finite animations; never advance clocks or sleep.
export function installInteractions() {
  const s = window.__leakSoak;
  s.frame = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  s.settleUI = async () => {
    await s.drain(); await s.frame();
    await Promise.all(document.getAnimations().filter(a => Number.isFinite(a.effect.getComputedTiming().endTime)).map(a => a.finished.catch(() => {})));
    await s.frame(); await s.drain();
  };
  s.scroll = async (element, top) => {
    const target = Math.max(0, Math.min(top, element.scrollHeight - element.clientHeight));
    if (element.scrollTop !== target) {
      const scrolled = new Promise(resolve => (element === document.scrollingElement ? document : element).addEventListener('scroll', resolve, {once: true}));
      element.scrollTo({top: target, behavior: 'instant'}); await scrolled;
    }
    await s.settleUI();
    if (Math.abs(element.scrollTop - target) > 1) throw Error('Scroll did not reach requested position');
  };
  s.click = selector => {const element = document.querySelector(selector); if (!element) throw Error(`Missing ${selector}`); element.click();};
  s.evidence = {cycles: 0, scrolls: 0, speakers: 0, details: 0, voiceJoins: 0, micTests: 0};
  window.addEventListener('infuser-presence-motion', () => s.evidence.speakers++);
  s.chatCycle = async () => {
    if (document.querySelector('#chat-panel').hidden) {s.click('#chat-toggle'); await s.settleUI();}
    const log = document.querySelector('#chat-log');
    if (log.scrollHeight <= log.clientHeight) throw Error('Chat fixture does not overflow');
    const visible = () => {const r = log.getBoundingClientRect(); return [...log.querySelectorAll('.chat-message')].filter(el => {const b = el.getBoundingClientRect(); return b.bottom > r.top && b.top < r.bottom;}).map(el => el.dataset.id).join(',');};
    const before = visible(), max = log.scrollHeight - log.clientHeight;
    // One real scroll gesture per cycle, through top/middle/latest. Open/close
    // every ten gestures rather than making each gesture a separate chat visit.
    await s.scroll(log, log.scrollTop >= max - 1 ? 0 : log.scrollTop === 0 ? max / 2 : max);
    if (visible() === before) throw Error('Visible chat history did not change');
    s.evidence.scrolls++; s.evidence.cycles++;
    if (s.evidence.cycles % 10 === 0) {
      s.click('#chat-toggle'); await s.settleUI();
      if (!document.querySelector('#chat-panel').hidden || document.querySelector('#chat-pointers').childElementCount) throw Error('Chat did not close');
    }
  };

  s.gridCycle = async () => {
    const grid = document.scrollingElement;
    if (grid.scrollHeight <= innerHeight) throw Error('Game grid fixture does not overflow');
    await s.scroll(grid, grid.scrollHeight / 2); await s.scroll(grid, grid.scrollHeight); await s.scroll(grid, 0); s.evidence.scrolls += 3;
    s.click('.game[data-app="game-0"] .game-open'); await s.settleUI();
    if (!document.querySelector('#game-detail').open) throw Error('Game details did not open');
    await Promise.all([...document.querySelectorAll('#game-detail img')].map(img => img.decode()));
    s.click('#close-game-detail'); await s.settleUI();
    if (document.querySelector('#game-detail').open) throw Error('Game details did not close');
    s.evidence.details++; s.evidence.cycles++;
  };
  s.voiceCycle = async () => {
    const state = () => document.querySelector('#voice-toggle').dataset.state;
    s.click('#voice-toggle'); await s.drain();
    if (state() !== 'on' || !s.room) throw Error('Voice did not join');
    const active = s.voiceAccounting.counts();
    if (!active.contexts || active.tracks < 3 || active.controls !== 1) throw Error('Voice media/control paths were not exercised');
    s.evidence.voiceJoins++;
    s.click('#voice-mixer-toggle'); await s.settleUI();
    s.room.emit('ActiveSpeakersChanged', [{identity: 'remote'}]);
    for (const [index, muted, deafened] of [[0, true, false], [1, true, true], [0, false, false]]) {
      document.querySelectorAll('#voice-mixer .vm-action')[index].click(); await s.drain();
      const badge = document.querySelector('.voice-split').dataset;
      if (badge.muted !== String(muted) || badge.deafened !== String(deafened)) throw Error('Mute/deafen controls did not apply');
    }
    for (const value of ['150', '100']) {
      const slider = document.querySelector('#voice-mixer input[aria-label="Voice volume"]');
      slider.value = value; slider.dispatchEvent(new Event('change', {bubbles: true})); await s.drain();
    }
    s.room.emit('TrackUnsubscribed', s.room.remote, {}, s.room.peer);
    s.room.emit('ParticipantDisconnected', s.room.peer);
    s.room.emit('ParticipantConnected', s.room.peer);
    s.room.emit('TrackSubscribed', s.room.remote, {}, s.room.peer);
    s.room.emit('Reconnecting'); s.room.emit('Reconnected'); await s.drain();
    s.click('#voice-mixer .vm-leave'); await s.drain(); await s.settleUI();
    await s.voiceAccounting.closed();
    if (state() !== 'off' || s.room) throw Error('Voice did not leave');
    // Real microphone loopback/input processor, then stop via popover dismissal.
    s.click('#voice-mixer-toggle'); await s.settleUI();
    if (!document.querySelector('#voice-mixer .vm-advanced').open) s.click('#voice-mixer .vm-advanced > summary');
    await s.settleUI();
    s.click('#voice-mixer .vm-test'); await s.drain(); await s.frame();
    if (s.voiceAccounting.counts().tracks < 2) throw Error('Mic test did not capture/process audio');
    s.evidence.micTests++;
    s.click('#voice-mixer .vm-close'); await s.settleUI(); await s.voiceAccounting.closed();
    // Reconcile the departed remote roster through the normal status poll, so
    // every measurement has the same off/empty-roster UI (not a timer race).
    await s.hooks.voice(); await s.hooks['voice-refreshDevices']();
    s.click('#voice-mixer-toggle'); await s.settleUI();
    s.click('#voice-mixer .vm-close'); await s.settleUI();
    const idle = s.voiceAccounting.counts();
    if (Object.values(idle).some(Boolean)) throw Error(`Voice resources survived leave: ${JSON.stringify(idle)}`);
    s.evidence.cycles++;
  };
}

export async function interactionCycle(page, scenario) {
  // A failure deadline, not a delay or retry. Missing teardown must fail rather
  // than hang forever waiting for an AudioContext or scroll event.
  let timer;
  try {
    await Promise.race([
      page.evaluate(name => window.__leakSoak[`${name}Cycle`](), scenario),
      new Promise((_, reject) => {timer = setTimeout(() => reject(Error(`${scenario} cycle did not complete in 15 seconds`)), 15000);}),
    ]);
  } finally {clearTimeout(timer);}
}
export async function assertInteractions(page, scenario, cycles) {
  const e = await page.evaluate(() => window.__leakSoak.evidence);
  assert.equal(e.cycles, cycles);
  if (scenario === 'chat') {assert.equal(e.scrolls, cycles); assert(e.speakers > 0, 'Real speaker movement must run');}
  if (scenario === 'grid') {assert.equal(e.scrolls, cycles * 3); assert.equal(e.details, cycles);}
  if (scenario === 'voice') {assert.equal(e.voiceJoins, cycles); assert.equal(e.micTests, cycles);}
  assert.equal(await page.evaluate(() => window.__leakSoak.frames()), 0, 'No live animation-frame callback remains at rest');
}
