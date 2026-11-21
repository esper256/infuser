import test from 'node:test';
import assert from 'node:assert/strict';
import {
  clampPercent,
  normalizeSettings,
  percentToGain,
  effectivePeerGain,
  chimeGainScale,
  rmsToMeterLevel,
  createVoiceInputProcessor,
  createOutputController,
} from '../web/voice-audio.js';

test('percentToGain follows perceptual curve with unity at 100% and +8 dB boost at 200%', () => {
  assert.equal(percentToGain(0), 0);
  assert.equal(percentToGain(50), 0.25);
  assert.equal(percentToGain(100), 1);
  assert.equal(percentToGain(150), 1.75);
  assert.equal(percentToGain(200), 2.5);
  assert.equal(percentToGain(999), 2.5);
  assert.equal(percentToGain(-10), 0);
});

test('effectivePeerGain and chimeGainScale honor mute, deafen, and boost', () => {
  assert.equal(effectivePeerGain({ outputVolume: 100, peerVolume: 100 }), 1);
  assert.equal(effectivePeerGain({ outputVolume: 100, peerVolume: 200 }), 2.5);
  assert.equal(effectivePeerGain({ outputVolume: 50, peerVolume: 100 }), 0.25);
  assert.equal(effectivePeerGain({ outputVolume: 100, peerVolume: 150, peerMuted: true }), 0);
  assert.equal(effectivePeerGain({ outputVolume: 100, peerVolume: 150, deafened: true }), 0);
  assert.equal(chimeGainScale({ outputVolume: 100, deafened: false }), 1);
  assert.equal(chimeGainScale({ outputVolume: 200, deafened: false }), 1);
  assert.equal(chimeGainScale({ outputVolume: 50, deafened: false }), 0.25);
  assert.equal(chimeGainScale({ outputVolume: 100, deafened: true }), 0);
});

test('normalizeSettings clamps ranges and prunes default peer overrides', () => {
  const normalized = normalizeSettings({
    input_gain: 250,
    output_volume: -5,
    auto_gain_control: false,
    peer_volumes: {
      defaultUser: { volume: 100, muted: false },
      loudFriend: { volume: 60, muted: false },
      quietFriend: { volume: 185, muted: true },
    },
  });
  assert.equal(clampPercent('abc'), 100);
  assert.equal(normalized.input_gain, 200);
  assert.equal(normalized.output_volume, 0);
  assert.equal(normalized.auto_gain_control, false);
  assert.equal(normalized.noise_suppression, true);
  assert.equal(normalized.echo_cancellation, true);
  assert.deepEqual(normalized.peer_volumes, {
    loudFriend: { volume: 60, muted: false },
    quietFriend: { volume: 185, muted: true },
  });
});

test('rmsToMeterLevel detects silence, speech, and clipping', () => {
  const silent = rmsToMeterLevel(new Float32Array(64));
  assert.equal(silent.level, 0);
  assert.equal(silent.clipping, false);

  const mid = new Float32Array(64).fill(0.25);
  const midReading = rmsToMeterLevel(mid);
  const expectedDb = 20 * Math.log10(0.25);
  assert.equal(midReading.db, expectedDb);
  assert.equal(midReading.level, (expectedDb + 60) / 54);
  assert.equal(midReading.clipping, false);

  const hot = new Float32Array(64).fill(0.99);
  const hotReading = rmsToMeterLevel(hot);
  assert.equal(hotReading.level, 1);
  assert.equal(hotReading.clipping, true);
});

test('createVoiceInputProcessor wires gain, limiter, analyser, loopback, and cleans up', async () => {
  const edges = [];
  const gainTargets = [];
  let stopped = false;
  const mockProcessedTrack = { stop() { stopped = true; } };
  const makeNode = name => ({
    name,
    connect(target) { edges.push([name, target.name || 'dest']); },
    disconnect() { edges.push(['disconnect', name]); },
  });
  const ctx = {
    currentTime: 1.5,
    destination: { name: 'speakers' },
    createMediaStreamSource: () => makeNode('source'),
    createGain: () => {
      const node = makeNode('gain');
      node.gain = {
        value: 1,
        setTargetAtTime(v, t, tc) { gainTargets.push([v, t, tc]); this.value = v; },
      };
      return node;
    },
    createDynamicsCompressor: () => ({
      ...makeNode('limiter'),
      threshold: { value: 0 },
      knee: { value: 0 },
      ratio: { value: 0 },
      attack: { value: 0 },
      release: { value: 0 },
    }),
    createAnalyser: () => ({
      ...makeNode('analyser'),
      fftSize: 0,
      smoothingTimeConstant: 0,
      getFloatTimeDomainData(buf) { buf.fill(0.25); },
    }),
    createMediaStreamDestination: () => ({
      ...makeNode('streamDest'),
      stream: { getAudioTracks: () => [mockProcessedTrack] },
    }),
  };

  const proc = createVoiceInputProcessor(1.5);
  await proc.init({ track: {}, audioContext: ctx });
  assert.equal(proc.processedTrack, mockProcessedTrack);
  assert.deepEqual(edges, [
    ['source', 'gain'],
    ['gain', 'limiter'],
    ['limiter', 'analyser'],
    ['analyser', 'streamDest'],
  ]);

  proc.setGain(2.25);
  assert.deepEqual(gainTargets, [[2.25, 1.5, 0.025]]);
  assert.equal(proc.getMeter().level, (20 * Math.log10(0.25) + 60) / 54);

  proc.setLoopback(true);
  assert(edges.some(([a, b]) => a === 'gain' && b === 'speakers'));

  await proc.destroy();
  assert.equal(stopped, true);
  assert.equal(proc.processedTrack, undefined);
});

test('createOutputController keeps <=100% on native element and switches to Web Audio only for boost', async () => {
  const ctxCalls = [];
  const volumes = [];
  const plugins = [];
  const sinks = [];
  const fakeTrack = {
    setVolume(v) { volumes.push(v); },
    setAudioContext(c) { ctxCalls.push(Boolean(c)); },
    setWebAudioPlugins(p) { plugins.push(p.length); },
    async setSinkId(id) { sinks.push(id); },
  };
  const fakeCtx = {
    createDynamicsCompressor: () => ({
      threshold: { value: 0 },
      knee: { value: 0 },
      ratio: { value: 0 },
      attack: { value: 0 },
      release: { value: 0 },
      disconnect() {},
    }),
  };
  const ctrl = createOutputController({ getAudioContext: () => fakeCtx });
  ctrl.register('peer-1', fakeTrack, 0.5);
  assert.deepEqual(ctxCalls, []);
  assert.deepEqual(volumes, [0.5]);

  // Boost above 1.0 lazily engages Web Audio + limiter.
  ctrl.apply('peer-1', 2.0);
  assert.deepEqual(ctxCalls, [true]);
  assert.deepEqual(plugins, [1]);
  assert.deepEqual(volumes, [0.5, 2.0]);

  // Lowering back to <=1.0 releases Web Audio so browser AEC uses native playback.
  ctrl.apply('peer-1', 0.8);
  assert.deepEqual(ctxCalls, [true, false]);
  assert.deepEqual(plugins, [1, 0]);
  assert.deepEqual(volumes, [0.5, 2.0, 0.8]);

  await ctrl.setSinkId('headset-out');
  assert.deepEqual(sinks, ['headset-out']);
  ctrl.clear();
});
