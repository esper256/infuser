import {sessionFetch} from './session-fetch.js';
// Preview PeerJS 1.5.5 adapter. PeerJS owns serialization and WebRTC connections.
const base = '/_infuser/api/v1/peers';
async function request(path, body, method = 'POST') {
  const response = await sessionFetch(base + path, {
    method, credentials:'same-origin', signal:AbortSignal.timeout(5000),
    ...(body === undefined ? {} : {headers:{'Content-Type':'application/json'},body:JSON.stringify(body)})
  });
  const result = await response.json();
  if (!response.ok) throw Object.assign(new Error(result.error || 'Shared session unavailable'),{status:response.status});
  return result;
}
export function create(options) { return enter(true, options); }
export function join(options) { return enter(false, options); }
async function enter(createRoom, {Peer, relayOnly = false, sessionId, playerId, joinable = createRoom,
  connectionOptions = {}, onConnection = () => {}, onState = () => {}, onEnd = () => {}} = {}) {
  if (typeof Peer !== 'function') throw new TypeError('Pass the project’s PeerJS Peer constructor.');
  if (typeof relayOnly !== 'boolean' || typeof joinable !== 'boolean') throw new TypeError('relayOnly and joinable must be booleans');
  if (createRoom && (sessionId !== undefined || playerId !== undefined)) throw new TypeError('Create takes no session or player target');
  if (sessionId !== undefined && !/^[a-f0-9]{32}$/.test(sessionId)) throw new TypeError('invalid sessionId');
  if (playerId !== undefined && (sessionId === undefined || typeof playerId !== 'string' || !/^[a-zA-Z0-9-]{1,96}$/.test(playerId))) throw new TypeError('playerId requires a sessionId');
  // Only public DataConnection settings. Connection identity/configuration is platform-owned.
  if (Object.keys(connectionOptions).some(k => !['serialization','reliable','label','metadata'].includes(k))) throw new TypeError('unsupported PeerJS connection option');
  const start = performance.now(), startWall = Date.now();
  const initial = await request('', {...(createRoom ? {create:true} : {}), adapter:'peerjs',connection_id:crypto.randomUUID(),relay_only:relayOnly,expected_epoch:sessionId,expected_player:playerId,joinable});
  let live = true, lastPoll = start, state = initial, peer, timer, healthTimer, refreshed = start;
  let lastPollWall = startWall;
  let iceServers = initial.ice_servers;
  const links = new Map(), wire = id => `i-${initial.epoch}-${id}`;
  const own = wire(initial.self_id), host = wire(initial.host_id);
  const active = () => live && Math.max(performance.now() - lastPoll, Date.now() - lastPollWall) < initial.lease_ms;
  const allowed = id => state.peers.some(p => wire(p.id) === id) && id !== own && (initial.role === 'host' || id === host);
  const current = connection => active() && links.get(connection.peer)?.connection === connection && allowed(connection.peer);
  const indicator = status => { if (window.parent !== window) window.parent.postMessage({type:'infuser-network',status},location.origin); };
  const config = {iceServers,iceTransportPolicy:initial.ice_transport_policy};
  function report() {
    if (!active()) return;
    const connected = [...links].filter(([,l]) => l.connection.open).map(([id]) => id);
    const expected = initial.role === 'host' ? state.peers.length - 1 : 1;
    indicator(connected.length === expected ? 'connected' : 'connecting');
    onState({...state,connected});
  }
  function drop(id) { const link = links.get(id); if (!link) return; links.delete(id); link.connection.close(); }
  function end(reason) {
    if (!live) return;
    live = false; clearTimeout(timer); clearInterval(watchdog); clearInterval(healthTimer);
    for (const id of links.keys()) drop(id);
    peer?.destroy(); indicator('ended');
    request('/' + initial.self_id,undefined,'DELETE').catch(() => {});
    onEnd(reason);
  }
  const watchdog = setInterval(() => { if (!active()) end('Shared session expired. Join again.'); },500);
  function attach(connection) {
    if (!active() || !allowed(connection.peer) || (initial.role !== 'host' && connection.peer !== host)) { connection.close(); return; }
    drop(connection.peer);
    const link = {connection,created:performance.now(),disconnected:0};
    links.set(connection.peer,link);
    // Keep real DataConnections and their message formats. Fence callbacks/sends
    // synchronously too: queued data can run before a timer after laptop sleep.
    const emit = connection.emit, send = connection.send;
    connection.emit = function(event,...args) {
      if (event === 'data' && !current(connection)) return false;
      return emit.call(this,event,...args);
    };
    connection.send = function(...args) {
      if (!current(connection) || !connection.open || connection.dataChannel?.bufferedAmount > 1048576) return false;
      try { send.apply(this,args); return true; } catch { return false; }
    };
    connection.on('open',report);
    connection.on('close',() => { if (links.get(connection.peer) === link) links.delete(connection.peer); report(); });
    connection.on('error',() => { if (links.get(connection.peer) === link) drop(connection.peer); report(); });
    const player = state.peers.find(p => wire(p.id) === connection.peer);
    try { onConnection(connection,player); } catch (error) { end('Game connection handler failed.'); throw error; }
  }
  function createPeer() {
    if (!active()) return;
    const instance = new Peer(own, {host:location.hostname,port:Number(location.port || (location.protocol === 'https:' ? 443 : 80)),secure:location.protocol === 'https:',
      path:initial.peerjs.path,key:initial.peerjs.key,token:crypto.randomUUID(),config,pingInterval:5000,debug:0});
    peer = instance;
    instance.on('connection',connection => { if (instance !== peer || initial.role !== 'host') connection.close(); else attach(connection); });
    instance.on('call',call => call.close());
    instance.on('error',error => {
      if (instance !== peer || !active()) return;
      if (['invalid-id','invalid-key','browser-incompatible','ssl-unavailable'].includes(error.type)) end('PeerJS connection could not start.');
    });
  }
  async function poll() {
    if (!live) return;
    const sent = performance.now(), sentWall = Date.now();
    try {
      const next = await request('/' + initial.self_id,undefined,'GET');
      if (!active() || next.epoch !== initial.epoch) { end('Shared session ended.'); return; }
      lastPoll = sent; lastPollWall = sentWall; state = next;
      for (const id of links.keys()) if (!allowed(id)) drop(id);
      const expiry = Math.min(...iceServers.map(s => s.expires_at || Infinity));
      if (performance.now() - refreshed > 1200000 || Date.now()/1000 > expiry - 120) {
        const renewed = await request('/' + initial.self_id + '/ice');
        if (!active()) return;
        iceServers = renewed.ice_servers; config.iceServers = iceServers; refreshed = performance.now();
        // PeerJS retains this config object for future connections. Update existing
        // ones, then members rebuild their data link with fresh relay credentials.
        for (const {connection} of links.values()) {
          try { connection.peerConnection?.setConfiguration(config); } catch { /* closed */ }
        }
        if (initial.role === 'member') for (const id of links.keys()) drop(id);
      }
      if (peer?.destroyed) createPeer();
      else if (peer?.disconnected) { try { peer.reconnect(); } catch { /* retry while lease is live */ } }
      if (initial.role === 'member' && peer?.open) {
        const link = links.get(host), time = performance.now();
        if (link) {
          const status = link.connection.peerConnection?.connectionState;
          if (['failed','disconnected','closed'].includes(status)) link.disconnected ||= time;
          else link.disconnected = 0;
        }
        if (!link || (link.disconnected && time - link.disconnected > 5000) || (!link.connection.open && time - link.created > 15000)) {
          drop(host); attach(peer.connect(host,connectionOptions));
        }
      }
      report();
    } catch (error) {
      if ([401,403,404,409,410,422].includes(error.status)) end(error.message);
    }
    if (live) timer = setTimeout(poll,1000);
  }
  const room = {
    id:own, hostId:host, role:initial.role, epoch:initial.epoch,
    get active() {return active();}, get peers() {return state.peers.map(p=>({...p,peerjs_id:wire(p.id)}));},
    get connections() {return [...links.values()].map(l=>l.connection);},
    async setJoinable(value) {
      if (typeof value !== 'boolean') throw new TypeError('joinable must be a boolean');
      if (!active()) throw new Error('Shared session ended.');
      const next = await request('/' + initial.self_id + '/joinability',{epoch:initial.epoch,joinable:value},'PUT');
      if (active()) {state = next;report();}
    },
    close() {end('Left the shared session.');},
    async stats() {
      const result=[];
      for (const [id,{connection}] of links) {
        const pc=connection.peerConnection;if (!pc) continue;
        const reports=await pc.getStats();
        for (const r of reports.values()) if (r.type==='transport' && r.selectedCandidatePairId) {
          const pair=reports.get(r.selectedCandidatePairId);
          result.push({peer:id,state:pc.connectionState,local:reports.get(pair.localCandidateId)?.candidateType,remote:reports.get(pair.remoteCandidateId)?.candidateType,bytesSent:pair.bytesSent,bytesReceived:pair.bytesReceived,round_trip_ms:typeof pair.currentRoundTripTime==='number'?Math.round(pair.currentRoundTripTime*1000):null});
        }
      }
      return result;
    }
  };
  try {createPeer();} catch (error) {end('PeerJS initialization failed.');throw error;}
  healthTimer=setInterval(async()=>{try {if(active()){const connections=await room.stats();if(active())await request('/'+initial.self_id+'/health',{connections});}} catch {/* diagnostics only */}},5000);
  report();timer=setTimeout(poll,0);
  return room;
}
