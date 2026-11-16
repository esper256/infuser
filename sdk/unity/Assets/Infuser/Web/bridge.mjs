// Unity adapter preview. Platform behavior stays in the versioned browser SDK.
async function connect() {
  const path = '/_infuser/sdk/v1.js';
  if (![...document.scripts].some(s => s.getAttribute('src') === path)) return null;
  const {Infuser} = await import(path);
  await Infuser.ready; // Hosted authentication failure is never standalone mode.
  return Infuser;
}

export function createBridge(send, loadSdk = connect) {
  const sdk = Promise.resolve().then(loadSdk);
  // Loading is asynchronous, but a failed connection is reported by each call.
  sdk.catch(() => {});
  let peer = null, socket = null, generation = 0, joining = false, disposed = false, connectingAbort = null;
  const emit = (event, value) => { if (!disposed) send({event, json: JSON.stringify(value ?? null)}); };
  const failure = (code, message) => Object.assign(new Error(message), {code});
  const requirePeer = () => {
    if (!peer?.active) throw failure('not_connected', 'Join multiplayer first.');
    return peer;
  };
  const errorEvent = e => emit('session_error', {error:e.detail?.message || 'Return to the Library and launch again.', code:e.detail?.code, status:e.detail?.status});
  globalThis.addEventListener?.('infuser-session-error', errorEvent);
  async function close(api) {
    generation++;
    connectingAbort?.abort();
    const oldPeer = peer, oldSocket = socket;
    peer = socket = null;
    oldPeer?.close();
    if (oldSocket) await api.sessions.disconnect(oldSocket);
  }
  async function operation(op, a) {
    const api = await sdk;
    if (disposed) throw failure('disposed', 'The Unity adapter has stopped.');
    if (op === 'ready') return api ? {hosted:true, ...await api.ready} : {hosted:false};
    if (!api) throw failure('not_hosted', 'Use the game’s standalone save or networking path.');
    switch (op) {
      case 'presence': return api.presence.get();
      case 'launch_intent': return api.launchIntent.consume();
      case 'cloud.load': return api.cloud.load(a.group);
      case 'cloud.read': return api.cloud.read(a.group, a.revision);
      case 'cloud.history': return api.cloud.history(a.group);
      case 'cloud.commit': return api.cloud.commit(a.group, a.checkpoint);
      case 'cloud.resolve': return api.cloud.resolve(a.group, a.checkpoint);
      // JsonUtility cannot deserialize dictionaries. This convenience view is
      // for a group declaring exactly one JSON file; raw cloud.* handles others.
      case 'save.load':
      case 'save.read': {
        const v = op === 'save.load' ? await api.cloud.load(a.group) : await api.cloud.read(a.group, a.revision);
        const exists = Object.hasOwn(v.files, a.file);
        if (v.revision !== null && (!exists || Object.keys(v.files).length !== 1 || v.formats?.[a.file] === 'binary'))
          throw failure('save_layout', 'This helper requires one declared JSON file per save group.');
        return {revision:v.revision, exists, json:exists ? JSON.stringify(v.files[a.file]) : '', summary:v.summary, created_at:v.created_at ?? 0, conflicts:v.conflicts};
      }
      case 'save.commit': {
        const checkpoint = {expectedRevision:a.expectedRevision || null, commitId:a.commitId,
          files:{[a.file]:JSON.parse(a.json)}, summary:a.summary || ''};
        return a.candidateRevision ? api.cloud.resolve(a.group, {...checkpoint, candidateRevision:a.candidateRevision}) : api.cloud.commit(a.group, checkpoint);
      }
      case 'peer.join': {
        if (peer || socket || joining) throw failure('already_connected', 'Leave the current multiplayer session first.');
        joining = true;
        const current = ++generation;
        const event = (name, value) => { if (current === generation) emit(name, value); };
        try {
          const options = {relayOnly:a.relayOnly === true, joinable:a.joinable === true,
            onState:v => event('peer_state', v), onMessage:(from,data) => event('peer_message', {from,json:JSON.stringify(data)}),
            onEnd:reason => { if (current === generation) { peer = null; event('peer_end', {reason}); } }};
          if (a.intent) {
            if (a.intent.kind !== 'join_friend' || !a.intent.session_id || !a.intent.player_id) throw failure('invalid_target', 'Invalid Join Friend target.');
            options.sessionId = a.intent.session_id; options.playerId = a.intent.player_id;
          } else if (a.sessionId) options.sessionId = a.sessionId;
          const room = await api.peers.join(options);
          if (current !== generation || disposed) { room.close(); throw failure('cancelled', 'Join cancelled.'); }
          peer = room;
          return {id:room.id, hostId:room.hostId, role:room.role, epoch:room.epoch};
        } finally { joining = false; }
      }
      case 'peer.send': return {sent:requirePeer().send(a.to, JSON.parse(a.json))};
      case 'peer.broadcast': requirePeer().broadcast(JSON.parse(a.json)); return {};
      case 'peer.joinable': return {joinable:await requirePeer().setJoinable(a.joinable)};
      case 'socket.connect': {
        if (peer || socket || joining) throw failure('already_connected', 'Leave the current multiplayer session first.');
        joining = true;
        const current = ++generation;
        const abort = new AbortController();
        connectingAbort = abort;
        try {
          const s = await api.sessions.connect({...a, binaryType:'arraybuffer', signal:abort.signal});
          if (current !== generation || disposed) { await api.sessions.disconnect(s); throw failure('cancelled', 'Join cancelled.'); }
          socket = s;
          s.addEventListener('message', e => {
            if (current !== generation) return;
            emit('socket_message', typeof e.data === 'string' ? {text:e.data} : api.cloud.encodeBinary(new Uint8Array(e.data)));
          });
          s.addEventListener('close', e => { if (current === generation) { socket = null; emit('socket_end', {code:e.code, reason:e.reason}); } });
          s.addEventListener('error', () => { if (current === generation) emit('socket_error', {error:'Game connection failed.'}); });
          return s.infuser;
        } finally { joining = false; if (connectingAbort === abort) connectingAbort = null; }
      }
      case 'socket.send': {
        if (!socket || socket.readyState !== 1 || socket.bufferedAmount > 65536) return {sent:false};
        const data = a.base64 == null ? a.text : api.cloud.decodeBinary({base64:a.base64});
        if (typeof data !== 'string' && !(data instanceof Uint8Array)) throw failure('invalid_message', 'Supply text or base64 bytes.');
        try { socket.send(data); return {sent:true}; } catch { return {sent:false}; }
      }
      case 'network.close': await close(api); return {};
      default: throw failure('unknown_operation', 'Unknown Unity adapter operation: ' + op);
    }
  }
  return {
    async call(id, op, json) {
      try { const value = await operation(op, JSON.parse(json || '{}')); if (!disposed) send({id, ok:true, json:JSON.stringify(value ?? null)}); }
      catch (e) { if (!disposed) send({id, ok:false, error:e.message, code:e.code || 'request_failed', status:e.status || 0}); }
    },
    dispose() {
      disposed = true; generation++; peer?.close(); peer = null;
      connectingAbort?.abort();
      // Close the connection immediately even if the SDK request is interrupted.
      const oldSocket = socket; socket = null; oldSocket?.close();
      if (oldSocket) sdk.then(api => api?.sessions.disconnect(oldSocket)).catch(() => {});
      globalThis.removeEventListener?.('infuser-session-error', errorEvent);
    }
  };
}
globalThis.InfuserUnity = {createBridge};
