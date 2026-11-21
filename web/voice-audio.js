// Shared voice gain math, Web Audio input processor, and hybrid AEC-safe output controller.
export const MAX_PERCENT = 200;

export const DEFAULT_SETTINGS = Object.freeze({
  input_gain: 100,
  output_volume: 100,
  auto_gain_control: true,
  noise_suppression: true,
  echo_cancellation: true,
  peer_volumes: Object.freeze({}),
});

export function clampPercent(value) {
  const num = Number(value);
  if (!Number.isFinite(num)) return 100;
  return Math.max(0, Math.min(MAX_PERCENT, Math.round(num)));
}

export function normalizeSettings(raw) {
  const src = raw && typeof raw === 'object' ? raw : {};
  const peers = {};
  if (src.peer_volumes && typeof src.peer_volumes === 'object') {
    for (const [id, entry] of Object.entries(src.peer_volumes)) {
      if (!id || typeof entry !== 'object' || !entry) continue;
      const volume = clampPercent(entry.volume ?? 100);
      const muted = Boolean(entry.muted);
      if (volume !== 100 || muted) peers[id] = { volume, muted };
    }
  }
  return {
    input_gain: clampPercent(src.input_gain ?? 100),
    output_volume: clampPercent(src.output_volume ?? 100),
    auto_gain_control: src.auto_gain_control !== false,
    noise_suppression: src.noise_suppression !== false,
    echo_cancellation: true,
    peer_volumes: peers,
  };
}

// Perceptual curve: 0% -> silence, 1..100% -> quadratic taper, 100..200% -> +8 dB boost (up to 2.5x).
export function percentToGain(pct) {
  const p = clampPercent(pct);
  if (p <= 0) return 0;
  if (p <= 100) {
    const unit = p / 100;
    return unit * unit;
  }
  return 1 + ((p - 100) / 100) * 1.5;
}

export function effectivePeerGain({ outputVolume = 100, peerVolume = 100, peerMuted = false, deafened = false } = {}) {
  if (deafened || peerMuted) return 0;
  return percentToGain(outputVolume) * percentToGain(peerVolume);
}

export function chimeGainScale({ outputVolume = 100, deafened = false } = {}) {
  if (deafened) return 0;
  return Math.min(1, percentToGain(outputVolume));
}

export function rmsToMeterLevel(samples) {
  if (!samples || !samples.length) return { level: 0, db: -100, clipping: false };
  let sum = 0;
  let peak = 0;
  for (let i = 0; i < samples.length; i++) {
    const v = samples[i];
    sum += v * v;
    const a = Math.abs(v);
    if (a > peak) peak = a;
  }
  const rms = Math.sqrt(sum / samples.length);
  const db = rms > 1e-6 ? 20 * Math.log10(rms) : -100;
  const level = Math.max(0, Math.min(1, (db + 60) / 54));
  return { level, db, clipping: peak >= 0.98 };
}

function createSoftLimiter(ctx) {
  if (typeof ctx.createDynamicsCompressor !== 'function') return null;
  const limiter = ctx.createDynamicsCompressor();
  limiter.threshold.value = -3;
  limiter.knee.value = 6;
  limiter.ratio.value = 12;
  limiter.attack.value = 0.003;
  limiter.release.value = 0.15;
  return limiter;
}

function setGainSmooth(param, ctx, target) {
  const safe = Math.max(0, Number.isFinite(target) ? target : 1);
  if (typeof param.setTargetAtTime === 'function' && Number.isFinite(ctx?.currentTime)) {
    param.setTargetAtTime(safe, ctx.currentTime, 0.025);
  } else {
    param.value = safe;
  }
}

// LiveKit LocalAudioTrack TrackProcessor for post-capture gain, soft limiting, and live metering.
export function createVoiceInputProcessor(initialGain = 1) {
  let ctx = null;
  let source = null;
  let gainNode = null;
  let limiter = null;
  let analyser = null;
  let destination = null;
  let loopbackGain = null;
  let meterBuffer = null;
  let currentGain = Math.max(0, Number.isFinite(initialGain) ? initialGain : 1);
  let loopbackEnabled = false;

  const processor = {
    name: 'infuser-voice-input',
    processedTrack: undefined,

    async init({ track, audioContext }) {
      await processor.destroy();
      ctx = audioContext;
      const stream = typeof MediaStream === 'function' ? new MediaStream([track]) : { getTracks: () => [track] };
      source = ctx.createMediaStreamSource(stream);
      gainNode = ctx.createGain();
      gainNode.gain.value = currentGain;
      limiter = createSoftLimiter(ctx);
      analyser = typeof ctx.createAnalyser === 'function' ? ctx.createAnalyser() : null;
      if (analyser) {
        analyser.fftSize = 512;
        analyser.smoothingTimeConstant = 0.35;
        meterBuffer = new Float32Array(analyser.fftSize);
      }
      destination = ctx.createMediaStreamDestination();
      let tail = source;
      tail.connect(gainNode);
      tail = gainNode;
      if (limiter) {
        tail.connect(limiter);
        tail = limiter;
      }
      if (analyser) {
        tail.connect(analyser);
        tail = analyser;
      }
      tail.connect(destination);
      if (loopbackEnabled) connectLoopback(tail);
      processor.processedTrack = destination.stream.getAudioTracks()[0];
    },

    async restart(opts) {
      await processor.init(opts);
    },

    async destroy() {
      disconnectLoopback();
      for (const node of [source, gainNode, limiter, analyser, destination]) {
        try { node?.disconnect(); } catch { /* already disconnected */ }
      }
      try { processor.processedTrack?.stop?.(); } catch { /* ignore */ }
      processor.processedTrack = undefined;
      ctx = null;
      source = null;
      gainNode = null;
      limiter = null;
      analyser = null;
      destination = null;
      meterBuffer = null;
    },

    setGain(linearGain) {
      currentGain = Math.max(0, Number.isFinite(linearGain) ? linearGain : 1);
      if (gainNode && ctx) setGainSmooth(gainNode.gain, ctx, currentGain);
    },

    setLoopback(enabled) {
      loopbackEnabled = Boolean(enabled);
      if (!ctx || !gainNode) return;
      if (loopbackEnabled) connectLoopback(analyser || limiter || gainNode);
      else disconnectLoopback();
    },

    getMeter() {
      if (!analyser || !meterBuffer) return { level: 0, db: -100, clipping: false };
      analyser.getFloatTimeDomainData(meterBuffer);
      return rmsToMeterLevel(meterBuffer);
    },
  };

  function connectLoopback(fromNode) {
    disconnectLoopback();
    if (!ctx || !fromNode || !ctx.destination) return;
    loopbackGain = ctx.createGain();
    loopbackGain.gain.value = 0.85;
    fromNode.connect(loopbackGain);
    loopbackGain.connect(ctx.destination);
  }

  function disconnectLoopback() {
    if (!loopbackGain) return;
    try { loopbackGain.disconnect(); } catch { /* ignore */ }
    loopbackGain = null;
  }

  return processor;
}

// Hybrid output controller: native <audio> up to 100% (preserves Chromium AEC on speakers),
// switching lazily to LiveKit's Web Audio GainNode + soft limiter only when boosted > 100%.
export function createOutputController({ getAudioContext } = {}) {
  const entries = new Map();
  let sinkId = '';

  function applyEntry(entry, linearGain) {
    const safe = Math.max(0, Math.min(6.25, Number.isFinite(linearGain) ? linearGain : 1));
    entry.lastGain = safe;
    const { track } = entry;
    if (safe > 1.0 && typeof track.setAudioContext === 'function') {
      const ctx = getAudioContext?.();
      if (ctx) {
        if (!entry.usingWebAudio) {
          entry.limiter = createSoftLimiter(ctx);
          if (entry.limiter && typeof track.setWebAudioPlugins === 'function') {
            track.setWebAudioPlugins([entry.limiter]);
          }
          track.setAudioContext(ctx);
          entry.usingWebAudio = true;
        }
        track.setVolume(safe);
        return;
      }
      // Fallback if no AudioContext is available: cap at native element maximum.
      track.setVolume(1.0);
      return;
    }
    if (entry.usingWebAudio && typeof track.setAudioContext === 'function') {
      track.setAudioContext(undefined);
      if (typeof track.setWebAudioPlugins === 'function') track.setWebAudioPlugins([]);
      try { entry.limiter?.disconnect(); } catch { /* ignore */ }
      entry.limiter = null;
      entry.usingWebAudio = false;
    }
    if (typeof track.setVolume === 'function') {
      track.setVolume(Math.min(1.0, safe));
    }
  }

  return {
    register(identity, track, initialGain = 1.0) {
      const entry = { track, limiter: null, usingWebAudio: false, lastGain: initialGain };
      entries.set(identity, entry);
      if (sinkId && typeof track.setSinkId === 'function') {
        void Promise.resolve(track.setSinkId(sinkId)).catch(() => {});
      }
      applyEntry(entry, initialGain);
    },

    unregister(identity) {
      const entry = entries.get(identity);
      if (!entry) return;
      try { entry.limiter?.disconnect(); } catch { /* ignore */ }
      entries.delete(identity);
    },

    apply(identity, linearGain) {
      const entry = entries.get(identity);
      if (entry) applyEntry(entry, linearGain);
    },

    applyAll(resolveGain) {
      for (const [identity, entry] of entries) {
        applyEntry(entry, resolveGain(identity));
      }
    },

    async setSinkId(deviceId) {
      sinkId = deviceId || '';
      const tasks = [];
      for (const { track } of entries.values()) {
        if (typeof track.setSinkId === 'function') {
          tasks.push(Promise.resolve(track.setSinkId(sinkId)).catch(() => {}));
        }
      }
      await Promise.all(tasks);
    },

    clear() {
      for (const entry of entries.values()) {
        try { entry.limiter?.disconnect(); } catch { /* ignore */ }
      }
      entries.clear();
    },

    has(identity) {
      return entries.has(identity);
    },
  };
}
