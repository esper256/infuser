// Shared split pill control for voice chat (Library header + Game Player header).
export const headsetSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M4 13v-2a8 8 0 0 1 16 0v2M4 12H3v7h4v-7H4zm16 0h1v6h-4v-6h3zM20 18v1a3 3 0 0 1-3 3h-3"/><rect x="10" y="20" width="5" height="3" rx="1.5" fill="currentColor" stroke="none"/></svg>';
export const deafenSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M4 13v-2a8 8 0 0 1 16 0v2M4 12H3v7h4v-7H4zm16 0h1v6h-4v-6h3zM3 3l18 18"/></svg>';
export const micOnSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><rect x="9" y="2" width="6" height="11" rx="3"/><path d="M5 10a7 7 0 0 0 14 0M12 17v4M8 21h8"/></svg>';
export const micOffSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M9 5a3 3 0 0 1 6 0v5M5 10a7 7 0 0 0 11.9 5M12 17v4M8 21h8M3 3l18 18"/></svg>';
export const speakerOnSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M11 5L6 9H2v6h4l5 4V5zM15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14"/></svg>';
export const speakerOffSvg = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M11 5L6 9H2v6h4l5 4V5zM23 9l-6 6M17 9l6 6"/></svg>';
const caretSvg = '<svg viewBox="0 0 16 16" aria-hidden="true" focusable="false"><path d="M4.5 6.25 8 9.75l3.5-3.5"/></svg>';

export function createVoiceSplit({
  withToggle = true,
  controlsId = 'voice-mixer',
  onToggle = () => {},
  onOpen = () => {},
  onClose = () => {},
} = {}) {
  const root = document.createElement('div');
  root.className = 'voice-split voice-control';
  root.dataset.mode = withToggle ? 'split' : 'single';
  root.hidden = true;

  let expanded = false;
  let toggleBtn = null;
  let iconHost = null;
  const badge = document.createElement('span');
  badge.className = 'voice-split-badge';
  badge.setAttribute('aria-hidden', 'true');
  badge.hidden = true;
  badge.innerHTML = micOffSvg;

  const caretBtn = document.createElement('button');
  caretBtn.type = 'button';
  caretBtn.setAttribute('aria-haspopup', 'dialog');
  caretBtn.setAttribute('aria-expanded', 'false');
  caretBtn.setAttribute('aria-controls', controlsId);

  if (withToggle) {
    toggleBtn = document.createElement('button');
    toggleBtn.id = 'voice-toggle';
    toggleBtn.className = 'voice-split-toggle';
    toggleBtn.type = 'button';
    toggleBtn.setAttribute('aria-pressed', 'false');
    toggleBtn.setAttribute('aria-label', 'Voice chat off — turn on');
    iconHost = document.createElement('span');
    iconHost.className = 'voice-split-icon';
    iconHost.innerHTML = headsetSvg;
    toggleBtn.append(iconHost, badge);
    toggleBtn.addEventListener('click', () => onToggle());

    const divider = document.createElement('span');
    divider.className = 'voice-split-divider';
    divider.setAttribute('aria-hidden', 'true');

    caretBtn.id = 'voice-mixer-toggle';
    caretBtn.className = 'voice-split-caret';
    caretBtn.innerHTML = caretSvg;
    caretBtn.setAttribute('aria-label', 'Voice levels and settings');
    caretBtn.title = 'Voice levels and settings';

    root.append(toggleBtn, divider, caretBtn);
  } else {
    caretBtn.id = 'voice-levels';
    caretBtn.className = 'voice-split-caret voice-split-solo';
    iconHost = document.createElement('span');
    iconHost.className = 'voice-split-icon';
    iconHost.innerHTML = headsetSvg;
    const caret = document.createElement('span');
    caret.className = 'voice-caret';
    caret.setAttribute('aria-hidden', 'true');
    caret.innerHTML = caretSvg;
    caretBtn.append(iconHost, caret, badge);
    caretBtn.setAttribute('aria-label', 'Voice controls');
    caretBtn.title = 'Voice controls';
    root.append(caretBtn);
  }

  let suppressNextOpen = false;
  caretBtn.addEventListener('pointerdown', () => {
    suppressNextOpen = expanded;
  });
  caretBtn.addEventListener('click', () => {
    if (suppressNextOpen) {
      suppressNextOpen = false;
      onClose();
      return;
    }
    if (expanded) onClose();
    else onOpen();
  });

  function setState({
    visible = true,
    on = false,
    live = false,
    state = 'off',
    muted = false,
    deafened = false,
    syncing = false,
    connected = false,
    disabled = false,
    expanded: nextExpanded,
  } = {}) {
    if (typeof nextExpanded === 'boolean') {
      expanded = nextExpanded;
    }
    const isOn = Boolean(on);
    const isDeaf = Boolean(isOn && deafened);
    const isMuted = Boolean(isOn && (muted || deafened));
    const showMuteBadge = Boolean(isOn && muted && !deafened);

    root.hidden = !visible;
    root.dataset.on = String(isOn);
    root.dataset.live = String(Boolean(live));
    root.dataset.state = state;
    root.dataset.muted = String(isMuted);
    root.dataset.deafened = String(isDeaf);
    root.dataset.syncing = String(Boolean(syncing));
    root.dataset.connected = String(Boolean(connected));
    root.dataset.expanded = String(expanded);

    iconHost.innerHTML = isDeaf ? deafenSvg : headsetSvg;
    badge.hidden = !showMuteBadge;

    caretBtn.setAttribute('aria-expanded', String(expanded));
    caretBtn.dataset.muted = String(isMuted);
    caretBtn.dataset.deafened = String(isDeaf);
    caretBtn.dataset.syncing = String(Boolean(syncing));
    caretBtn.dataset.connected = String(Boolean(connected));

    if (toggleBtn) {
      toggleBtn.dataset.state = state;
      toggleBtn.dataset.muted = String(isMuted);
      toggleBtn.dataset.deafened = String(isDeaf);
      toggleBtn.dataset.syncing = String(Boolean(syncing));
      toggleBtn.setAttribute('aria-pressed', String(isOn));
      toggleBtn.disabled = Boolean(disabled);
      const toggleLabel =
        state === 'connecting'
          ? 'Voice chat connecting — turn off'
          : state === 'reconnecting'
            ? 'Voice chat reconnecting — turn off'
            : isDeaf
              ? 'Voice chat on (deafened) — turn off'
              : isMuted
                ? 'Voice chat on (muted) — turn off'
                : isOn
                  ? 'Voice chat on — turn off'
                  : 'Voice chat off — turn on';
      toggleBtn.title = toggleLabel;
      toggleBtn.setAttribute('aria-label', toggleLabel);

      const caretLabel = isDeaf
        ? 'Voice levels and settings (deafened)'
        : isMuted
          ? 'Voice levels and settings (muted)'
          : 'Voice levels and settings';
      caretBtn.title = caretLabel;
      caretBtn.setAttribute('aria-label', caretLabel);
    } else {
      const soloLabel = syncing
        ? 'Voice controls (updating…)'
        : isDeaf
          ? 'Voice controls (deafened)'
          : isMuted
            ? 'Voice controls (muted)'
            : connected
              ? 'Voice controls'
              : 'Voice controls (connecting)';
      caretBtn.title = soloLabel;
      caretBtn.setAttribute('aria-label', soloLabel);
    }
  }

  return { root, toggleBtn, caretBtn, setState };
}
