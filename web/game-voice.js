// Slim voice mixer for the isolated game player header bar.
// Communicates strictly via same-origin /_infuser/voice; never acquires a microphone.
import { clampPercent } from '/_infuser/voice-audio.js';

const micOnSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><rect x="9" y="2" width="6" height="11" rx="3"/><path d="M5 10a7 7 0 0 0 14 0M12 17v4M8 21h8"/></svg>';
const micOffSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M9 5a3 3 0 0 1 6 0v5M5 10a7 7 0 0 0 11.9 5M12 17v4M8 21h8M3 3l18 18"/></svg>';
const headsetSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M4 13v-2a8 8 0 0 1 16 0v2M4 12H3v7h4v-7H4zm16 0h1v6h-4v-6h3zM20 18v1a3 3 0 0 1-3 3h-3"/><rect x="10" y="20" width="5" height="3" rx="1.5" fill="currentColor" stroke="none"/></svg>';
const deafenSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M4 13v-2a8 8 0 0 1 16 0v2M4 12H3v7h4v-7H4zm16 0h1v6h-4v-6h3zM3 3l18 18"/></svg>';
const speakerOnSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M11 5L6 9H2v6h4l5 4V5zM15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14"/></svg>';
const speakerOffSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M11 5L6 9H2v6h4l5 4V5zM23 9l-6 6M17 9l6 6"/></svg>';

export function initGameVoice({ header, anchorBefore, onPopoverChange = () => {}, onCloseFocus = () => {} }) {
  const muteBtn = document.createElement('button');
  muteBtn.id = 'voice-mute';
  muteBtn.type = 'button';
  muteBtn.hidden = true;
  muteBtn.innerHTML = micOnSvg;

  const levelsBtn = document.createElement('button');
  levelsBtn.id = 'voice-levels';
  levelsBtn.type = 'button';
  levelsBtn.hidden = true;
  levelsBtn.setAttribute('aria-haspopup', 'dialog');
  levelsBtn.setAttribute('aria-expanded', 'false');
  levelsBtn.setAttribute('aria-controls', 'game-voice-mixer');
  levelsBtn.innerHTML = headsetSvg + '<span class="voice-caret" aria-hidden="true">▾</span>';

  anchorBefore.before(muteBtn, levelsBtn);

  const mixer = document.createElement('section');
  mixer.id = 'game-voice-mixer';
  mixer.setAttribute('role', 'dialog');
  mixer.setAttribute('aria-label', 'Voice controls');
  mixer.hidden = true;
  header.append(mixer);

  let state = null;
  let open = false;
  let alive = true;
  let polling = false;
  let dragging = false;
  let pendingPatch = null;
  let patchTimer = null;
  let sending = false;
  let lastRenderSig = '';
  let pollTimer = null;

  function peerSetting(playerId) {
    const raw = state?.peer_volumes?.[playerId];
    return {
      volume: clampPercent(raw?.volume ?? 100),
      muted: Boolean(raw?.muted),
    };
  }

  function setOpen(next, restoreFocus = false) {
    if (open === next) return;
    open = next;
    mixer.hidden = !open;
    levelsBtn.setAttribute('aria-expanded', String(open));
    onPopoverChange(open);
    if (open) {
      lastRenderSig = '';
      renderMixer();
      schedulePoll();
      void poll();
    } else {
      schedulePoll();
      if (restoreFocus) onCloseFocus();
    }
  }

  function paintBar() {
    const active = Boolean(state?.configured && state?.active);
    muteBtn.hidden = !active;
    levelsBtn.hidden = !active;
    if (!active) {
      if (open) setOpen(false, false);
      return;
    }
    const muted = Boolean(state.mic_muted || state.deafened);
    const deafened = Boolean(state.deafened);
    muteBtn.dataset.muted = String(muted);
    muteBtn.setAttribute('aria-pressed', String(muted));
    muteBtn.innerHTML = muted ? micOffSvg : micOnSvg;
    const muteLabel = deafened ? 'Deafened — unmute microphone' : muted ? 'Microphone muted — unmute' : 'Mute microphone';
    muteBtn.title = muteLabel;
    muteBtn.setAttribute('aria-label', muteLabel);

    levelsBtn.dataset.deafened = String(deafened);
    levelsBtn.dataset.connected = String(Boolean(state.connected));
    levelsBtn.innerHTML = (deafened ? deafenSvg : headsetSvg) + '<span class="voice-caret" aria-hidden="true">▾</span>';
    const levelsLabel = deafened ? 'Voice controls (deafened)' : 'Voice controls';
    levelsBtn.title = levelsLabel;
    levelsBtn.setAttribute('aria-label', levelsLabel);
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
      const res = await fetch('/_infuser/voice', {
        method: 'PATCH',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
        signal: AbortSignal.timeout(5000),
      });
      if (res.ok) {
        const updated = await res.json();
        if (state && updated.revision >= (state.revision || 0)) {
          state = {
            ...state,
            revision: updated.revision,
            input_gain: updated.input_gain,
            output_volume: updated.output_volume,
            peer_volumes: updated.peer_volumes || {},
            mic_muted: updated.mic_muted,
            deafened: updated.deafened,
          };
          paintBar();
          if (open && !dragging) renderMixer();
        }
      } else if (res.status === 409) {
        await poll();
      }
    } catch {
      /* Next poll reconciles state. */
    } finally {
      sending = false;
      if (pendingPatch) void flushPatch();
    }
  }

  function queuePatch(patch, immediate = false) {
    // Apply optimistically to local snapshot so rapid clicks feel instant.
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
      await fetch('/_infuser/voice', {
        method: 'DELETE',
        credentials: 'same-origin',
        signal: AbortSignal.timeout(5000),
      });
    } catch { /* ignore */ }
    if (state) state.active = false;
    setOpen(false, true);
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
    const sig = JSON.stringify([
      state.connected,
      state.mic_muted,
      state.deafened,
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
    title.textContent = state.connected ? 'Voice chat' : 'Voice chat · Connecting';
    const closeBtn = document.createElement('button');
    closeBtn.type = 'button';
    closeBtn.className = 'gvm-close';
    closeBtn.textContent = '×';
    closeBtn.setAttribute('aria-label', 'Close voice controls');
    closeBtn.onclick = () => setOpen(false, true);
    head.append(title, closeBtn);
    mixer.append(head);

    const quick = document.createElement('div');
    quick.className = 'gvm-actions';
    const muted = Boolean(state.mic_muted || state.deafened);
    const deafened = Boolean(state.deafened);

    const qMute = document.createElement('button');
    qMute.type = 'button';
    qMute.className = 'gvm-action';
    qMute.dataset.active = String(muted);
    qMute.setAttribute('aria-pressed', String(muted));
    qMute.innerHTML = (muted ? micOffSvg : micOnSvg) + `<span>${muted ? 'Unmute' : 'Mute'}</span>`;
    qMute.onclick = () => {
      if (deafened) queuePatch({ mic_muted: false, deafened: false }, true);
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
      queuePatch({ deafened: !deafened }, true);
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
  }

  muteBtn.addEventListener('click', () => {
    if (!state?.active) return;
    if (state.deafened) queuePatch({ mic_muted: false, deafened: false }, true);
    else queuePatch({ mic_muted: !state.mic_muted }, true);
    if (open) {
      lastRenderSig = '';
      renderMixer();
    }
    onCloseFocus();
  });

  levelsBtn.addEventListener('click', () => {
    setOpen(!open, open);
  });

  document.addEventListener('pointerdown', event => {
    if (!open) return;
    if (mixer.contains(event.target) || levelsBtn.contains(event.target) || muteBtn.contains(event.target)) return;
    setOpen(false, false);
  });

  mixer.addEventListener('keydown', event => {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      setOpen(false, true);
    }
  });

  async function poll() {
    if (polling || !alive) return;
    polling = true;
    try {
      const res = await fetch('/_infuser/voice', {
        credentials: 'same-origin',
        signal: AbortSignal.timeout(5000),
      });
      if (!res.ok) {
        if ([401, 403, 404, 410].includes(res.status)) {
          state = { configured: false, active: false };
          paintBar();
        }
        return;
      }
      const next = await res.json();
      if (!pendingPatch && !dragging) {
        state = next;
        paintBar();
        if (open) renderMixer();
      } else if (state && next.active) {
        // Keep participants list fresh without clobbering active slider drag.
        state.participants = next.participants;
        state.connected = next.connected;
      }
      if (!next.configured) {
        clearInterval(pollTimer);
        pollTimer = null;
      }
    } catch {
      /* Transient error; retry on next tick. */
    } finally {
      polling = false;
    }
  }

  function schedulePoll() {
    clearInterval(pollTimer);
    if (!alive || state?.configured === false) return;
    const ms = open ? 2500 : state?.active ? 5000 : 10000;
    pollTimer = setInterval(() => {
      if (!document.hidden) void poll();
    }, ms);
  }

  document.addEventListener('visibilitychange', () => {
    if (!document.hidden) void poll();
  });
  addEventListener('pagehide', () => {
    alive = false;
    clearInterval(pollTimer);
    clearTimeout(patchTimer);
  });

  void poll().then(schedulePoll);
  return {
    isOpen: () => open,
    close: (restoreFocus = false) => setOpen(false, restoreFocus),
  };
}
