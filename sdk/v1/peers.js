// Experimental WebRTC star topology. Keep game rules in the game, not this transport.
// One host per room; host departure ends the session. This is not the SDK 1.0 contract.
async function request(path, body, method = 'POST') {
  const response = await fetch('/_infuser/api/v1/peers' + path, {
    method, credentials: 'same-origin', signal: AbortSignal.timeout(5000),
    ...(body === undefined ? {} : {headers: {'Content-Type':'application/json'}, body: JSON.stringify(body)})
  });
  const result = await response.json();
  if (!response.ok) throw Object.assign(new Error(result.error || 'Peer session unavailable'), {status:response.status});
  return result;
}
export function create(options) { return enter(true, options); }
export function join(options) { return enter(false, options); }
async function enter(createRoom, {relayOnly = false, sessionId, playerId, joinable, onState = () => {}, onMessage = () => {}, onEnd = () => {}} = {}) {
  if (createRoom && (sessionId !== undefined || playerId !== undefined)) throw new TypeError('Create takes no session or player target');
  if (sessionId !== undefined && (typeof sessionId !== 'string' || !/^[a-f0-9]{32}$/.test(sessionId))) throw new TypeError('sessionId must be an observed session ID');
  if (joinable !== undefined && typeof joinable !== 'boolean') throw new TypeError('joinable must be a boolean');
  if (playerId !== undefined && (sessionId === undefined || typeof playerId !== 'string' || !/^[a-zA-Z0-9-]{1,96}$/.test(playerId))) throw new TypeError('playerId requires a sessionId and an opaque player ID');
  const started = performance.now(), startedWall = Date.now();
  const initial = await request('', {...(createRoom ? {create:true} : {}), connection_id: crypto.randomUUID(), relay_only: relayOnly, expected_epoch:sessionId, expected_player:playerId, joinable:joinable ?? (createRoom ? true : undefined)});
  let state = initial, live = true, lastPoll = started, cursor = 0, refreshed = started, timer, healthTimer;
  let lastPollWall = startedWall;
  let iceServers = initial.ice_servers;
  const links = new Map();
  const config = () => ({iceServers, iceTransportPolicy: initial.ice_transport_policy});
  // Some browsers pause the monotonic clock during system sleep. Either clock
  // expiring must fence packets before a delayed timer or poll can renew access.
  const ownsLease = () => live && Math.max(performance.now() - lastPoll, Date.now() - lastPollWall) < state.lease_ms;
  const current = (id, link) => ownsLease() && links.get(id) === link;
  const indicator = status => { if (window.parent !== window) window.parent.postMessage({type:'infuser-network',status}, location.origin); };
  const report = () => {
    if (!ownsLease()) return;
    const connected = [...links].filter(([, l]) => l.channel?.readyState === 'open').map(([id]) => id);
    indicator(connected.length === state.peers.length - 1 ? 'connected' : 'connecting');
    onState({...state, connected});
  };
  function drop(id) { const link = links.get(id); if (!link) return; links.delete(id); link.pc.close(); }
  function end(reason) {
    if (!live) return;
    live = false; clearTimeout(timer); clearInterval(watchdog); clearInterval(healthTimer); indicator('ended');
    for (const id of links.keys()) drop(id);
    request('/' + initial.self_id, undefined, 'DELETE').catch(() => {});
    onEnd(reason);
  }
  // Fence a disconnected old host before the controller can appoint another one.
  const watchdog = setInterval(() => { if (!ownsLease()) end('Connection lost. Join the shared session again.'); }, 500);
  function channel(link, dc, id) {
    if (!current(id, link)) { dc.close(); return; }
    link.channel = dc;
    dc.onopen = dc.onclose = () => { if (current(id, link)) report(); };
    dc.onmessage = event => {
      if (!ownsLease() || links.get(id) !== link || typeof event.data !== 'string' || event.data.length > 65536) return;
      let data; try { data = JSON.parse(event.data); } catch { return; }
      onMessage(id, data);
    };
  }
  function signal(id, link, payload) {
    if (!current(id, link)) return Promise.resolve();
    return request('/' + initial.self_id + '/signals', {epoch: initial.epoch, to:id, payload});
  }
  function connect(id, generation) {
    drop(id);
    const pc = new RTCPeerConnection(config());
    const link = {pc, generation, announced:false, candidates:[], outgoing:Promise.resolve(), created:performance.now(), disconnected:0};
    links.set(id, link);
    const sendCandidate = candidate => {
      // Preserve trickle order and never leave rejected promises unhandled.
      link.outgoing = link.outgoing.then(() => signal(id, link, {type:'candidate', generation, candidate})).catch(() => {});
    };
    link.flush = () => { link.announced = true; for (const c of link.candidates.splice(0)) sendCandidate(c); };
    pc.onicecandidate = ({candidate}) => { if (candidate && current(id, link)) { const c = candidate.toJSON(); if (link.announced) sendCandidate(c); else link.candidates.push(c); } };
    pc.ondatachannel = ({channel: dc}) => channel(link, dc, id);
    pc.onconnectionstatechange = () => {
      if (!current(id, link)) return;
      if (pc.connectionState === 'connected') link.disconnected = 0;
      else if (!link.disconnected) link.disconnected = performance.now();
      report();
    };
    return link;
  }
  async function offer(id) {
    if (!ownsLease()) return;
    const link = connect(id, crypto.randomUUID());
    channel(link, link.pc.createDataChannel('infuser-game'), id);
    const description = await link.pc.createOffer();
    if (!current(id, link)) return;
    await link.pc.setLocalDescription(description);
    if (!current(id, link)) return;
    await signal(id, link, {type:'offer', generation:link.generation, description:link.pc.localDescription.toJSON()});
    if (!current(id, link)) return;
    link.flush();
  }
  async function receive({from, payload}) {
    if (!ownsLease()) return;
    if (!state.peers.some(p => p.id === from)) return;
    if (typeof payload.generation !== 'string' || payload.generation.length > 64) return;
    let link = links.get(from);
    if (payload.type === 'offer' && initial.role === 'member' && from === initial.host_id) {
      link = connect(from, payload.generation);
      await link.pc.setRemoteDescription(payload.description);
      if (!current(from, link)) return;
      const description = await link.pc.createAnswer();
      if (!current(from, link)) return;
      await link.pc.setLocalDescription(description);
      if (!current(from, link)) return;
      await signal(from, link, {type:'answer', generation:link.generation, description:link.pc.localDescription.toJSON()});
      if (!current(from, link)) return;
      link.flush();
    } else if (link?.generation === payload.generation) {
      if (payload.type === 'answer' && initial.role === 'host' && link.pc.signalingState === 'have-local-offer') {
        await link.pc.setRemoteDescription(payload.description);
        for (const c of (link.pending || [])) {
          if (!current(from, link)) return;
          await link.pc.addIceCandidate(c);
        }
        link.pending = [];
      } else if (payload.type === 'candidate') {
        if (link.pc.remoteDescription) await link.pc.addIceCandidate(payload.candidate);
        else { link.pending ||= []; if (link.pending.length < 32) link.pending.push(payload.candidate); }
      }
    }
  }
  async function poll() {
    if (!live) return;
    const sent = performance.now(), sentWall = Date.now();
    try {
      const next = await request('/' + initial.self_id + '?after=' + cursor, undefined, 'GET');
      if (!ownsLease()) { end('Session expired. Join again.'); return; }
      if (next.epoch !== initial.epoch) { end('Shared session ended.'); return; }
      lastPoll = sent; lastPollWall = sentWall; state = next;
      if (typeof next.network_path === 'string' && window.parent !== window) window.parent.postMessage({type:'infuser-path', label: next.network_path}, location.origin);
      for (const id of links.keys()) if (!state.peers.some(p => p.id === id)) drop(id);
      for (const message of next.signals) {
        try { await receive(message); } catch { /* A malformed or obsolete offer cannot stop the polling lease. */ }
        cursor = message.sequence;
      }
      if (initial.role === 'host') {
        for (const peer of state.peers) {
          if (peer.id === initial.self_id) continue;
          const link = links.get(peer.id), time = performance.now();
          if (!link || (link.disconnected && time - link.disconnected > 5000) || (link.channel?.readyState !== 'open' && time - link.created > 15000)) {
            await offer(peer.id);
          }
        }
      }
      if (ownsLease() && performance.now() - refreshed > 1200000) {
        const refreshedIce = await request('/' + initial.self_id + '/ice');
        if (!ownsLease()) return;
        iceServers = refreshedIce.ice_servers;
        refreshed = performance.now();
        if (initial.role === 'host') for (const id of links.keys()) await offer(id);
      }
      report();
    } catch (error) {
      if ([401,403,404,409,410].includes(error.status)) end(error.message);
    }
    if (live) timer = setTimeout(poll, 1000);
  }
  const room = {
    id:initial.self_id, hostId:initial.host_id, role:initial.role, epoch:initial.epoch,
    get active() { return ownsLease(); },
    get peers() { return state.peers; },
    get acceptingPlayers() { return state.accepting_players; },
    async setJoinable(joinable) {
      if (typeof joinable !== 'boolean') throw new TypeError('joinable must be a boolean');
      if (!ownsLease()) throw new Error('Shared session ended.');
      const next = await request('/' + initial.self_id + '/joinability', {epoch:initial.epoch, joinable}, 'PUT');
      if (live) { state.accepting_players = next.accepting_players; report(); }
      return next.accepting_players;
    },
    send(to, data) {
      if (!ownsLease()) return false;
      const dc = links.get(to)?.channel, message = JSON.stringify(data);
      if (!dc || dc.readyState !== 'open' || dc.bufferedAmount > 65536 || message.length > 65536) return false;
      // A ready channel can close or exhaust its buffer before send completes.
      try { dc.send(message); return true; } catch { return false; }
    },
    broadcast(data) { for (const id of links.keys()) room.send(id, data); },
    close() { end('Left the shared session.'); },
    async stats() {
      const result = [];
      for (const [id, link] of links) {
        const reports = await link.pc.getStats();
        for (const report of reports.values()) {
          if (report.type !== 'transport' || !report.selectedCandidatePairId) continue;
          const pair = reports.get(report.selectedCandidatePairId);
          result.push({peer:id, state:link.pc.connectionState, local:reports.get(pair.localCandidateId)?.candidateType, remote:reports.get(pair.remoteCandidateId)?.candidateType, bytesSent:pair.bytesSent, bytesReceived:pair.bytesReceived, round_trip_ms:typeof pair.currentRoundTripTime === 'number' ? Math.round(pair.currentRoundTripTime * 1000) : null});
        }
      }
      return result;
    }
  };
  // Operator diagnostics are separate from the player's quiet title-bar indicator.
  healthTimer = setInterval(async () => {
    if (!ownsLease()) return;
    try {
      const connections = await room.stats();
      if (ownsLease()) await request('/' + initial.self_id + '/health', {connections});
    } catch { /* Diagnostics cannot interrupt play. */ }
  }, 5000);
  report(); timer = setTimeout(poll, 0);
  return room;
}
