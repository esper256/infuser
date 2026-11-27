// Library-only navigation. Games own their input, bindings and prompts.
import {automaticLayout, connectedPads, layouts, preference} from './gamepads.js';

function buttonMask(pad) {
  return pad.buttons.reduce((mask, button, i) => i < 17 && button.pressed ? mask | (1 << i) : mask, 0);
}

function direction(pad) {
  const down = index => pad.buttons[index]?.pressed;
  if (down(12)) return 'up';
  if (down(13)) return 'down';
  if (down(14)) return 'left';
  if (down(15)) return 'right';
  const [x = 0, y = 0] = pad.axes;
  if (Math.max(Math.abs(x), Math.abs(y)) < 0.65) return '';
  if (Math.abs(x) > Math.abs(y)) return x < 0 ? 'left' : 'right';
  return y < 0 ? 'up' : 'down';
}

// Use card bounds so different title lengths and responsive column counts work.
function neighbor(current, cards, move) {
  const horizontal = move === 'left' || move === 'right';
  const sign = move === 'left' || move === 'up' ? -1 : 1;
  const rect = button => button.closest('.game').getBoundingClientRect();
  const origin = rect(current);
  let best, distance = Infinity, fallback = current, fallbackDistance = Infinity;
  for (const card of cards) {
    const box = rect(card);
    const aligned = horizontal ? box.top < origin.bottom && box.bottom > origin.top :
      box.left < origin.right && box.right > origin.left;
    const dx = box.x + box.width / 2 - origin.x - origin.width / 2;
    const dy = box.y + box.height / 2 - origin.y - origin.height / 2;
    const forward = sign * (horizontal ? dx : dy);
    const side = Math.abs(horizontal ? dy : dx);
    if (forward <= 1) continue;
    const radius = Math.hypot(forward, side);
    if (radius < fallbackDistance) { fallback = card; fallbackDistance = radius; }
    if (!aligned || forward + 2 * side >= distance) continue;
    best = card;
    distance = forward + 2 * side;
  }
  return best || fallback;
}

export function initLibraryGamepad(activate) {
  const detail = document.getElementById('game-detail');
  const hints = document.createElement('div');
  hints.className = 'library-pad-hints';
  hints.setAttribute('aria-hidden','true');
  const history = new Map();
  let timer, owner, fastUntil;
  function hide() {
    if (owner === undefined) return;
    hints.remove();
    document.documentElement.classList.remove('library-pad-active');
    owner = undefined;
    for (const state of history.values()) state.repeat = Infinity;
  }
  function show(pad, text) {
    owner = pad.index;
    document.documentElement.classList.add('library-pad-active');
    if (hints.textContent !== text) hints.textContent = text;
    const parent = detail.open ? detail : document.body;
    if (hints.parentElement !== parent) parent.append(hints);
    if (matchMedia('(max-width: 640px)').matches) {
      const target = document.activeElement.closest('.game') || document.activeElement;
      const box = target.getBoundingClientRect();
      hints.classList.toggle('at-top', box.bottom > innerHeight - hints.offsetHeight - 20);
    }
  }
  function navigate(pad, move, accepted, back) {
    const otherDialog = 'dialog[open]:not(#game-detail)';
    const saved = preference(pad.id);
    const family = saved === 'auto' ? automaticLayout(pad.id) : saved;
    const confirm = family === 'nintendo' ? back : accepted;
    const cancel = family === 'nintendo' ? accepted : back;
    if (document.querySelector('[data-launching]')) {
      if (cancel) show(pad, 'Starting…');
      return;
    }
    const focused = document.activeElement;
    if (focused.matches('input, textarea, select, [contenteditable=true]')) {
      if (cancel) focused.blur();
      hide();
      return;
    }
    const dialog = focused.closest(otherDialog) || document.querySelector(otherDialog);
    const scope = dialog || (detail.open ? detail : document);
    const menu = scope.querySelector('.play-menu[open], .header-menu[open], .game-admin-menu[open], .feedback-actions[open]');
    if (cancel && menu) {
      menu.open = false;
      menu.querySelector('summary').focus();
    } else if (dialog) {
      if (cancel) dialog.close(); // Native modal close restores its opener.
      hide();
      return;
    } else if (document.querySelector(':popover-open, #chat-toggle[aria-expanded=true]')) {
      if (cancel) {
        const popover = document.querySelector(':popover-open');
        if (popover) popover.hidePopover();
        else {
          const chat = document.getElementById('chat-toggle');
          chat.click(); chat.focus();
        }
      }
      hide();
      return;
    } else if (!detail.open && menu) {
      hide();
      return;
    } else if (detail.open && cancel) {
      detail.close();
    } else if (detail.open) {
      const actions = detail.querySelector('.actions');
      if (!actions) { hide(); return; }
      const menu = actions.querySelector('.play-menu');
      const items = menu.open ? [...menu.querySelectorAll('button:enabled')] :
        [...actions.querySelectorAll(':scope > button:enabled, summary')];
      const index = items.indexOf(document.activeElement);
      const step = move === 'right' || move === 'down' ? 1 : -1;
      const next = items[index < 0 ? 0 : move ? Math.max(0, Math.min(items.length - 1, index + step)) : index];
      next?.focus();
      if (confirm && index >= 0 && next) {
        activate(next);
        if (next.tagName === 'SUMMARY' && menu.open) menu.querySelector('button:enabled')?.focus();
      }
    } else {
      const cards = [...document.querySelectorAll('#visible-games .game-open')].filter(button => button.getClientRects().length);
      if (!cards.length) { hide(); return; }
      const current = document.activeElement;
      if (!cards.includes(current)) cards[0].focus();
      else if (confirm) {
        activate(current);
        detail.querySelector('.actions > button:enabled, .actions summary')?.focus();
      } else if (move) neighbor(current, cards, move).focus();
    }
    if (document.querySelector(otherDialog)) { hide(); return; }
    const target = document.activeElement;
    const action = !detail.open ? 'Open' : target?.matches('summary') ? 'Options' :
      target?.matches('.actions > button') ? 'Play' : target?.closest('.play-menu-items') ? 'Select' : '';
    const names = layouts[family], index = family === 'nintendo' ? 1 : 0;
    show(pad, [action && names[index] + ' · ' + action, detail.open && names[1-index] + ' · Back'].filter(Boolean).join('　'));
  }
  function seed(pad) {
    history.set(pad.index, {buttons:buttonMask(pad), move:direction(pad), repeat:Infinity});
  }
  function schedule() {
    clearTimeout(timer);
    timer = setTimeout(poll, performance.now() < fastUntil ? 40 : 250);
  }
  function poll() {
    if (document.hidden || !document.hasFocus()) { stop(); return; }
    const pads = connectedPads().filter(pad => pad.mapping === 'standard');
    if (!pads.length) { stop(); return; }
    for (const index of history.keys()) {
      if (!pads.some(pad => pad.index === index)) {
        history.delete(index);
        if (owner === index) hide();
      }
    }
    const now = performance.now();
    for (const pad of pads) {
      const state = history.get(pad.index);
      if (!state) { seed(pad); continue; }
      const buttons = buttonMask(pad);
      const pressed = buttons & ~state.buttons;
      const move = direction(pad);
      const changed = move && move !== state.move;
      const repeat = move && owner === pad.index && now >= state.repeat;
      if (buttons !== state.buttons || move !== state.move || repeat) fastUntil = now + 3000;
      state.buttons = buttons;
      state.move = move;
      if (changed || repeat) state.repeat = now + (changed ? 420 : 155);
      if (pressed || changed || repeat) navigate(pad, changed || repeat ? move : '', pressed & 1, pressed & 2);
    }
    schedule();
  }
  function stop() {
    clearTimeout(timer); timer = undefined;
    history.clear(); hide();
  }
  function wake(event) {
    if (document.hidden || !document.hasFocus()) { stop(); return; }
    const pads = connectedPads().filter(pad => pad.mapping === 'standard');
    if (!pads.length) { stop(); return; }
    for (const pad of pads) {
      if (!history.has(pad.index) || event?.gamepad?.index === pad.index) seed(pad);
    }
    fastUntil = performance.now() + 3000;
    schedule();
  }
  document.addEventListener('keydown', hide, true);
  document.addEventListener('pointerdown', hide, true);
  window.addEventListener('gamepadconnected', wake);
  window.addEventListener('gamepaddisconnected', wake);
  window.addEventListener('blur', stop);
  window.addEventListener('focus', wake);
  window.addEventListener('pagehide', stop);
  window.addEventListener('pageshow', wake);
  document.addEventListener('visibilitychange', wake);
  wake();
}
