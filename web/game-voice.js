// Slim voice mixer for the isolated game player header bar.
// Communicates strictly via same-origin /_infuser/voice using the player-page token; never acquires a microphone.
import { clampPercent } from '/_infuser/voice-audio.js';
import {
  createVoiceSplit,
  headsetSvg,
  deafenSvg,
  micOnSvg,
  micOffSvg,
  speakerOnSvg,
  speakerOffSvg,
} from '/_infuser/voice-split.js';

export function initGameVoice({
  header,
  anchorBefore,
  voiceToken = '',
  fetchFn = window.fetch.bind(window),
  onPopoverChange = () => {},
  onCloseFocus = () => {},
}) {
  if (!voiceToken) {
    return { isOpen: () => false, close: () => {} };
  }

  let state = null;
  let open = false;
  let alive = true;
  let fetching = false;
  let controlAbort = null;
  let controlRunning = false;
  let dragging = false;
  let pendingPatch = null;
  let patchTimer = null;
  let sending = false;
  let lastRenderSig = '';
  let restoreTarget = 'game';

  const mixer = document.createElement('section');
  mixer.id = 'game-voice-mixer';
  mixer.setAttribute('popover', 'auto');
  mixer.setAttribute('role', 'dialog');
  mixer.setAttribute('aria-label', 'Voice controls');
  document.body.append(mixer);

  const split = createVoiceSplit({
    withToggle: false,
    controlsId: 'game-voice-mixer',
    onOpen: () => openMixer(),
    onClose: () => closeMixer('caret'),
  });
  anchorBefore.before(split.root);

  const authHeaders = (extra = {}) => ({
    'X-Infuser-Player-Voice': voiceToken,
    ...extra,
  });

  function peerSetting(playerId) {
    const raw = state?.peer_volumes?.[playerId];
    return {
      volume: clampPercent(raw?.volume ?? 100),
      muted: Boolean(raw?.muted),
    };
  }

  function isMuteSyncing() {
    if (!state?.active) return false;
    const wantMuted = Boolean(state.mic_muted || state.deafened);
    const gotMuted = Boolean(state.applied_mic_muted || state.applied_deafened);
    const wantDeaf = Boolean(state.deafened);
    const gotDeaf = Boolean(state.applied_deafened);
    return wantMuted !== gotMuted || wantDeaf !== gotDeaf;
  }

  // Reflect what the Library tab has actually applied so the bar never claims
  // the microphone is muted while it is still live.
  function shownMicMuted() {
    if (!state?.active) return false;
    return Boolean(state.applied_mic_muted || state.applied_deafened);
  }
  function shownDeafened() {
    if (!state?.active) return false;
    return Boolean(state.applied_deafened);
  }

  function positionMixer() {
    if (!mixer.matches(':popover-open')) return;
    const headerRect = header.getBoundingClientRect();
    const width = Math.min(310, window.innerWidth - 20);
    const rightMargin = 10;
    const left = Math.max(10, window.innerWidth - width - rightMargin);
    mixer.style.width = `${width}px`;
    mixer.style.left = `${left}px`;
    mixer.style.top = `${Math.max(10, Math.round(headerRect.bottom + 6))}px`;
    mixer.style.maxHeight = `${Math.max(160, window.innerHeight - Math.round(headerRect.bottom + 18))}px`;
  }

  function openMixer() {
    if (!state?.configured || !state?.active) return;
    if (mixer.matches(':popover-open')) return;
    restoreTarget = 'game';
    lastRenderSig = '';
    mixer.showPopover();
    renderMixer();
    positionMixer();
    mixer.querySelector('.gvm-action')?.focus({ preventScroll: true });
    void fetchStatus();
  }

  function closeMixer(target = 'game') {
    restoreTarget = target;
    if (mixer.matches(':popover-open')) mixer.hidePopover();
  }

  mixer.addEventListener('beforetoggle', event => {
    open = event.newState === 'open';
    paintBar();
  });

  mixer.addEventListener('toggle', () => {
    open = mixer.matches(':popover-open');
    paintBar();
    onPopoverChange(open);
    if (open) {
      positionMixer();
    } else if (restoreTarget === 'caret') {
      split.caretBtn.focus({ preventScroll: true });
      restoreTarget = 'game';
    } else if (restoreTarget === 'game') {
      onCloseFocus();
    }
  });

  mixer.addEventListener('keydown', event => {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      closeMixer('caret');
    }
  });

  window.addEventListener('resize', positionMixer);

  function paintBar() {
    const active = Boolean(state?.configured && state?.active);
    if (!active && open) closeMixer('none');
    split.setState({
      visible: active,
      on: active,
      live: active && Boolean(state?.connected),
      state: active ? (state.connected ? 'on' : 'connecting') : 'off',
      muted: shownMicMuted(),
      deafened: shownDeafened(),
      syncing: isMuteSyncing(),
      connected: Boolean(state?.connected),
      expanded: open,
    });
  }

  function mergePatch(patch) {
    if (!pendingPatch) pendingPatch = {};
    for (const [key, value] of Object.entries(patch)) {
      if (key === 'peer_volumes') {
        pendingPatch.peer_volumes = { ...(pendingPatch.peer_volumes || {}) };
        for (const [pid, entry] of Object.entries(value)) {
          pendingPatch.peer_volumes[pid] = { ...(pendingPatch.peer_volumes[pid] || {}), ...entry };
        }
      } else {
        pendingPatch[key] = value;
      }
    }
  }

  async function flushPatch() {
    clearTimeout(patchTimer);
    patchTimer = null;
    if (!pendingPatch || sending || !alive) return;
    const payload = pendingPatch;
    pendingPatch = null;
    sending = true;
    try {
      const res = await fetchFn('/_infuser/voice', {
        method: 'PATCH',
        credentials: 'same-origin',
        headers: authHeaders({ 'Content-Type': 'application/json' }),
        body: JSON.stringify(payload),
        signal: AbortSignal.timeout(5000),
      });
      if (res.ok) {
        const updated = await res.json();
        const epochChanged = state && updated.epoch && state.epoch && updated.epoch !== state.epoch;
        if (state && (epochChanged || updated.revision >= (state.revision || 0))) {
          state = {
            ...state,
            epoch: updated.epoch ?? state.epoch,
            revision: updated.revision,
            applied_revision: updated.applied_revision ?? state.applied_revision,
            applied_mic_muted: updated.applied_mic_muted ?? state.applied_mic_muted,
            applied_deafened: updated.applied_deafened ?? state.applied_deafened,
            input_gain: updated.input_gain,
            output_volume: updated.output_volume,
            peer_volumes: updated.peer_volumes || {},
            mic_muted: updated.mic_muted,
            deafened: updated.deafened,
          };
          paintBar();
          if (open && !dragging) renderMixer();
          ensureControlLoop();
        }
      } else if (res.status === 409) {
        await fetchStatus();
      }
    } catch {
      /* Control loop or next focus/visibility refresh reconciles state. */
    } finally {
      sending = false;
      if (pendingPatch) void flushPatch();
    }
  }

  function queuePatch(patch, immediate = false) {
    if (state) {
      if (typeof patch.input_gain === 'number') state.input_gain = clampPercent(patch.input_gain);
      if (typeof patch.output_volume === 'number') state.output_volume = clampPercent(patch.output_volume);
      if (typeof patch.deafened === 'boolean') state.deafened = patch.deafened;
      if (typeof patch.mic_muted === 'boolean') {
        state.mic_muted = patch.mic_muted;
        if (!patch.mic_muted) state.deafened = false;
      }
      if (patch.peer_volumes) {
        state.peer_volumes = { ...(state.peer_volumes || {}) };
        for (const [pid, update] of Object.entries(patch.peer_volumes)) {
          const prev = peerSetting(pid);
          const next = {
            volume: update.volume !== undefined ? clampPercent(update.volume) : prev.volume,
            muted: update.muted !== undefined ? Boolean(update.muted) : prev.muted,
          };
          if (next.volume === 100 && !next.muted) delete state.peer_volumes[pid];
          else state.peer_volumes[pid] = next;
        }
      }
      paintBar();
    }
    mergePatch(patch);
    if (immediate) {
      void flushPatch();
    } else if (!patchTimer) {
      patchTimer = setTimeout(() => { void flushPatch(); }, 100);
    }
  }

  async function leaveVoice() {
    try {
      await fetchFn('/_infuser/voice', {
        method: 'DELETE',
        credentials: 'same-origin',
        headers: authHeaders(),
        signal: AbortSignal.timeout(5000),
      });
    } catch { /* ignore */ }
    if (state) state.active = false;
    closeMixer('game');
    paintBar();
  }

  function buildSliderRow({ label, value, onInput, onCommit, resetLabel }) {
    const row = document.createElement('div');
    row.className = 'gvm-slider-row';
    const top = document.createElement('div');
    top.className = 'gvm-slider-top';
    const name = document.createElement('span');
    name.textContent = label;
    const readout = document.createElement('button');
    readout.type = 'button';
    readout.className = 'gvm-pct';
    readout.textContent = `${value}%`;
    readout.title = resetLabel || 'Reset to 100%';
    top.append(name, readout);

    const input = document.createElement('input');
    input.type = 'range';
    input.min = '0';
    input.max = '200';
    input.step = '5';
    input.value = String(value);
    input.setAttribute('aria-label', label);
    input.className = 'gvm-range';

    input.addEventListener('pointerdown', () => { dragging = true; });
    const endDrag = () => { dragging = false; };
    input.addEventListener('pointerup', endDrag);
    input.addEventListener('pointercancel', endDrag);
    input.addEventListener('input', () => {
      const next = clampPercent(input.value);
      readout.textContent = `${next}%`;
      onInput(next);
    });
    input.addEventListener('change', () => {
      dragging = false;
      const next = clampPercent(input.value);
      readout.textContent = `${next}%`;
      onCommit(next);
    });
    const reset = () => {
      input.value = '100';
      readout.textContent = '100%';
      onCommit(100);
    };
    input.addEventListener('dblclick', reset);
    readout.addEventListener('click', reset);

    row.append(top, input);
    return row;
  }

  function renderMixer() {
    if (!open || !state?.active) return;
    const peers = (state.participants || []).filter(p => p.id !== state.self_id);
    const syncing = isMuteSyncing();
    const muted = shownMicMuted();
    const deafened = shownDeafened();
    const sig = JSON.stringify([
      state.connected,
      syncing,
      muted,
      deafened,
      state.input_gain,
      state.output_volume,
      state.peer_volumes,
      peers.map(p => [p.id, p.name]),
    ]);
    if (sig === lastRenderSig) return;
    lastRenderSig = sig;

    mixer.replaceChildren();
    const head = document.createElement('div');
    head.className = 'gvm-head';
    const title = document.createElement('strong');
    title.textContent = syncing
      ? 'Voice chat · Syncing…'
      : state.connected
        ? 'Voice chat'
        : 'Voice chat · Connecting';
    const closeBtn = document.createElement('button');
    closeBtn.type = 'button';
    closeBtn.className = 'gvm-close';
    closeBtn.textContent = '×';
    closeBtn.setAttribute('aria-label', 'Close voice controls');
    closeBtn.onclick = () => closeMixer('caret');
    head.append(title, closeBtn);
    mixer.append(head);

    const quick = document.createElement('div');
    quick.className = 'gvm-actions';

    const qMute = document.createElement('button');
    qMute.type = 'button';
    qMute.className = 'gvm-action';
    qMute.dataset.active = String(muted);
    qMute.setAttribute('aria-pressed', String(muted));
    qMute.innerHTML = (muted ? micOffSvg : micOnSvg) + `<span>${muted ? 'Unmute' : 'Mute'}</span>`;
    qMute.onclick = () => {
      if (state.deafened || deafened) queuePatch({ mic_muted: false, deafened: false }, true);
      else queuePatch({ mic_muted: !state.mic_muted }, true);
      lastRenderSig = '';
      renderMixer();
    };

    const qDeafen = document.createElement('button');
    qDeafen.type = 'button';
    qDeafen.className = 'gvm-action';
    qDeafen.dataset.active = String(deafened);
    qDeafen.setAttribute('aria-pressed', String(deafened));
    qDeafen.innerHTML = (deafened ? deafenSvg : headsetSvg) + `<span>${deafened ? 'Undeafen' : 'Deafen'}</span>`;
    qDeafen.onclick = () => {
      queuePatch({ deafened: !(state.deafened || deafened) }, true);
      lastRenderSig = '';
      renderMixer();
    };

    const qLeave = document.createElement('button');
    qLeave.type = 'button';
    qLeave.className = 'gvm-action gvm-leave';
    qLeave.textContent = 'Leave';
    qLeave.onclick = () => { void leaveVoice(); };

    quick.append(qMute, qDeafen, qLeave);
    mixer.append(quick);

    mixer.append(
      buildSliderRow({
        label: 'Microphone',
        value: clampPercent(state.input_gain ?? 100),
        onInput: v => queuePatch({ input_gain: v }, false),
        onCommit: v => queuePatch({ input_gain: v }, true),
      }),
      buildSliderRow({
        label: 'Voice volume',
        value: clampPercent(state.output_volume ?? 100),
        onInput: v => queuePatch({ output_volume: v }, false),
        onCommit: v => queuePatch({ output_volume: v }, true),
      })
    );

    if (peers.length) {
      const section = document.createElement('div');
      section.className = 'gvm-peers';
      const sub = document.createElement('p');
      sub.className = 'gvm-section-title';
      sub.textContent = 'People in voice';
      section.append(sub);
      for (const peer of peers) {
        const pref = peerSetting(peer.id);
        const card = document.createElement('div');
        card.className = 'gvm-peer';
        const top = document.createElement('div');
        top.className = 'gvm-peer-top';
        const label = document.createElement('span');
        label.className = 'gvm-peer-name';
        label.textContent = peer.name;

        const pctBtn = document.createElement('button');
        pctBtn.type = 'button';
        pctBtn.className = 'gvm-pct';
        pctBtn.textContent = `${pref.volume}%`;
        pctBtn.title = 'Reset to 100%';

        const pMute = document.createElement('button');
        pMute.type = 'button';
        pMute.className = 'gvm-peer-mute';
        pMute.dataset.muted = String(pref.muted);
        pMute.setAttribute('aria-pressed', String(pref.muted));
        pMute.setAttribute('aria-label', `${pref.muted ? 'Unmute' : 'Mute'} ${peer.name}`);
        pMute.title = pref.muted ? `Unmute ${peer.name}` : `Mute ${peer.name}`;
        pMute.innerHTML = pref.muted ? speakerOffSvg : speakerOnSvg;
        pMute.onclick = () => {
          queuePatch({ peer_volumes: { [peer.id]: { muted: !pref.muted } } }, true);
          lastRenderSig = '';
          renderMixer();
        };

        top.append(label, pctBtn, pMute);

        const slider = document.createElement('input');
        slider.type = 'range';
        slider.min = '0';
        slider.max = '200';
        slider.step = '5';
        slider.value = String(pref.volume);
        slider.className = 'gvm-range';
        slider.setAttribute('aria-label', `${peer.name} volume`);
        slider.addEventListener('pointerdown', () => { dragging = true; });
        slider.addEventListener('pointerup', () => { dragging = false; });
        slider.addEventListener('pointercancel', () => { dragging = false; });
        slider.addEventListener('input', () => {
          const next = clampPercent(slider.value);
          pctBtn.textContent = `${next}%`;
          queuePatch({ peer_volumes: { [peer.id]: { volume: next } } }, false);
        });
        slider.addEventListener('change', () => {
          dragging = false;
          const next = clampPercent(slider.value);
          pctBtn.textContent = `${next}%`;
          queuePatch({ peer_volumes: { [peer.id]: { volume: next } } }, true);
        });
        const resetPeer = () => {
          slider.value = '100';
          pctBtn.textContent = '100%';
          queuePatch({ peer_volumes: { [peer.id]: { volume: 100, muted: false } } }, true);
          lastRenderSig = '';
          renderMixer();
        };
        slider.addEventListener('dblclick', resetPeer);
        pctBtn.addEventListener('click', resetPeer);

        card.append(top, slider);
        section.append(card);
      }
      mixer.append(section);
    }

    const footer = document.createElement('div');
    footer.className = 'gvm-footer';
    const resetAll = document.createElement('button');
    resetAll.type = 'button';
    resetAll.className = 'gvm-reset';
    resetAll.textContent = 'Reset levels';
    resetAll.onclick = () => {
      const peerReset = {};
      for (const pid of Object.keys(state.peer_volumes || {})) {
        peerReset[pid] = { volume: 100, muted: false };
      }
      queuePatch({ input_gain: 100, output_volume: 100, peer_volumes: peerReset }, true);
      lastRenderSig = '';
      renderMixer();
    };
    footer.append(resetAll);
    mixer.append(footer);
    positionMixer();
  }

  function applySnapshot(next) {
    if (!pendingPatch && !dragging) {
      state = next;
      paintBar();
      if (open) renderMixer();
    } else if (state && next.active) {
      state.epoch = next.epoch ?? state.epoch;
      state.revision = next.revision ?? state.revision;
      state.roster = next.roster ?? state.roster;
      state.participants = next.participants;
      state.connected = next.connected;
      state.applied_revision = next.applied_revision;
      state.applied_mic_muted = next.applied_mic_muted;
      state.applied_deafened = next.applied_deafened;
      paintBar();
    }
  }

  function stopControlLoop() {
    controlAbort?.abort();
    controlAbort = null;
    controlRunning = false;
  }

  function ensureControlLoop() {
    if (!alive || document.hidden || controlRunning || !state || state.configured === false) return;
    void runControlLoop();
  }

  async function runControlLoop() {
    if (controlRunning) return;
    controlRunning = true;
    while (alive && !document.hidden && state && state.configured !== false) {
      const ac = new AbortController();
      controlAbort = ac;
      try {
        const params = new URLSearchParams({
          since: String(state.revision || 0),
          applied: String(state.applied_revision || 0),
          active: state.active ? '1' : '0',
          epoch: String(state.epoch || ''),
          roster: String(state.roster || ''),
        });
        const res = await fetchFn('/_infuser/voice/control?' + params.toString(), {
          credentials: 'same-origin',
          headers: authHeaders(),
          signal: AbortSignal.any([ac.signal, AbortSignal.timeout(30000)]),
        });
        if (!alive || ac.signal.aborted) break;
        if (res.status === 204) continue;
        if ([401, 403, 404, 410].includes(res.status)) {
          state = { configured: false, active: false };
          paintBar();
          break;
        }
        if (!res.ok) break;
        const next = await res.json();
        applySnapshot(next);
        if (!next.configured) break;
      } catch {
        // Exit without a retry timer; online/focus/visibilitychange or user action restarts.
        break;
      }
    }
    controlRunning = false;
  }

  async function fetchStatus() {
    if (fetching || !alive) return;
    fetching = true;
    try {
      const res = await fetchFn('/_infuser/voice', {
        credentials: 'same-origin',
        headers: authHeaders(),
        signal: AbortSignal.timeout(5000),
      });
      if (!res.ok) {
        if ([401, 403, 404, 410].includes(res.status)) {
          state = { configured: false, active: false };
          stopControlLoop();
          paintBar();
        }
        return;
      }
      const next = await res.json();
      applySnapshot(next);
      if (next.configured) ensureControlLoop();
      else stopControlLoop();
    } catch {
      /* Next focus/visibilitychange/online event refreshes state. */
    } finally {
      fetching = false;
    }
  }

  document.addEventListener('visibilitychange', () => {
    if (document.hidden) {
      stopControlLoop();
    } else {
      void fetchStatus();
    }
  });
  window.addEventListener('focus', () => {
    if (!document.hidden && !controlRunning) void fetchStatus();
  });
  window.addEventListener('online', () => {
    if (!document.hidden) void fetchStatus();
  });
  addEventListener('pagehide', () => {
    alive = false;
    stopControlLoop();
    clearTimeout(patchTimer);
  });

  void fetchStatus();
  return {
    isOpen: () => open,
    close: (restoreFocus = false) => closeMixer(restoreFocus ? 'game' : 'none'),
  };
}
