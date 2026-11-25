// Deliberately replace only the room/transport boundary. Infuser's voice module,
// input processor, output controller and Chromium media/audio objects stay real.
export const RoomEvent = Object.fromEntries(['ParticipantConnected', 'ParticipantDisconnected', 'TrackSubscribed', 'TrackUnsubscribed', 'ActiveSpeakersChanged', 'Reconnecting', 'Reconnected', 'Disconnected'].map(name => [name, name]));
export const DisconnectReason = {ROOM_DELETED: 'ROOM_DELETED'};
class LocalTrack {
  constructor(track) {this.mediaStreamTrack = track; this.isMuted = false;}
  async mute() {this.isMuted = true; this.mediaStreamTrack.enabled = false;}
  async unmute() {this.isMuted = false; this.mediaStreamTrack.enabled = true;}
  async setAudioContext(context) {this.audioContext = context;}
  async setProcessor(processor) {this.processor = processor; await processor.init({track: this.mediaStreamTrack, audioContext: this.audioContext});}
  stop() {this.mediaStreamTrack.stop();}
}
export async function createLocalAudioTrack() {
  const stream = await navigator.mediaDevices.getUserMedia({audio: true});
  return new LocalTrack(stream.getAudioTracks()[0]);
}
class RemoteTrack {
  constructor(track) {this.kind = 'audio'; this.track = track; this.elements = [];}
  attach() {const el = document.createElement('audio'); el.srcObject = new MediaStream([this.track]); this.elements.push(el); return el;}
  detach() {const elements = this.elements; this.elements = []; for (const el of elements) el.srcObject = null; return elements;}
  setVolume(gain) {for (const el of this.elements) el.volume = Math.min(1, gain);}
  setAudioContext(context) {this.context = context;}
  setWebAudioPlugins(plugins) {this.plugins = plugins;}
}
export class Room {
  constructor() {
    this.handlers = new Map(); this.remoteParticipants = new Map(); this.state = 'disconnected';
    this.localParticipant = {publishTrack: async mic => {
      this.remote = new RemoteTrack(mic.mediaStreamTrack.clone());
      this.peer = {identity: 'remote'}; this.remoteParticipants.set('remote', this.peer);
      this.emit(RoomEvent.ParticipantConnected, this.peer);
      this.emit(RoomEvent.TrackSubscribed, this.remote, {}, this.peer);
    }};
  }
  on(event, fn) {const handlers = this.handlers.get(event) || []; handlers.push(fn); this.handlers.set(event, handlers); return this;}
  once(event, fn) {return this.on(event, fn);}
  emit(event, ...args) {for (const fn of this.handlers.get(event) || []) fn(...args);}
  async connect() {this.state = 'connected'; window.__leakSoak.room = this;}
  async startAudio() {await Promise.all(this.remote.elements.map(el => el.play()));}
  async disconnect() {
    this.emit(RoomEvent.TrackUnsubscribed, this.remote, {}, this.peer);
    this.emit(RoomEvent.ParticipantDisconnected, this.peer);
    this.remote.track.stop(); this.state = 'disconnected';
    // Transport-owned resources only; never clear Infuser's callbacks/references.
    if (window.__leakSoak.room === this) window.__leakSoak.room = null;
  }
}

// Installed before modules load. Only live resources are retained, never history.
export function installVoiceAccounting() {
  const contexts = new Set(), tracks = new Set(), controls = new Set();
  const NativeAudio = window.AudioContext;
  window.AudioContext = class extends NativeAudio {
    constructor(...args) {
      super(...args); contexts.add(this);
      const changed = () => {if (this.state === 'closed') {contexts.delete(this); this.removeEventListener('statechange', changed);}};
      this.addEventListener('statechange', changed);
    }
  };
  const remember = track => {tracks.add(track); return track;};
  const gum = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
  navigator.mediaDevices.getUserMedia = async (...args) => {const stream = await gum(...args); stream.getTracks().forEach(remember); return stream;};
  const clone = MediaStreamTrack.prototype.clone, stop = MediaStreamTrack.prototype.stop;
  MediaStreamTrack.prototype.clone = function() {return remember(clone.call(this));};
  MediaStreamTrack.prototype.stop = function() {stop.call(this); tracks.delete(this);};
  const destination = AudioContext.prototype.createMediaStreamDestination;
  AudioContext.prototype.createMediaStreamDestination = function() {const node = destination.call(this); node.stream.getTracks().forEach(remember); return node;};
  const fetch = window.fetch.bind(window);
  window.fetch = (input, options = {}) => {
    if (new URL(input, location.href).pathname !== '/api/voice/control') return fetch(input, options);
    // A local long poll, completed only by the production abort on leave.
    return new Promise((resolve, reject) => {
      const signal = options.signal;
      const abort = () => {controls.delete(signal); signal.removeEventListener('abort', abort); reject(new DOMException('Aborted', 'AbortError'));};
      if (signal.aborted) abort(); else {controls.add(signal); signal.addEventListener('abort', abort, {once: true});}
    });
  };
  window.__leakSoak.voiceAccounting = {
    counts: () => ({contexts: contexts.size, tracks: tracks.size, controls: controls.size}),
    async closed() {
      // Wait for actual AudioContext state changes, including the production
      // chime's delayed close. No elapsed-time sleep or early test cleanup.
      await Promise.all([...contexts].map(ctx => ctx.state === 'closed' ? undefined : new Promise(resolve => {
        const changed = () => {if (ctx.state === 'closed') {ctx.removeEventListener('statechange', changed); resolve();}};
        ctx.addEventListener('statechange', changed);
      })));
    },
  };
}
