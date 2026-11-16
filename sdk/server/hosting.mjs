// Infuser dedicated-server adapter. No gameplay framing or second login.
import {createConnectionVerifier} from './connection.mjs';
import {randomUUID} from 'node:crypto';

export const sessionId = () => randomUUID().replaceAll('-','');

export class AdmissionError extends Error {
  constructor(code, message = code) {super(message); this.code = code;}
}
const codes = new Set(['session_ended','session_full','friend_left','session_unavailable','server_draining',
  'capacity_unavailable','resume_expired','already_connected','admission_rejected','session_exists']);

// Instantiate once. sockets is a ws.WebSocketServer({noServer:true}); admit reserves
// a seat atomically and returns {session, attach(socket), release(reason), suspend?}.
export function createInfuserServer({server, sockets, admit, sessions, onControl = () => {}, onError = error => console.warn('Infuser:', error.code || 'error', error.message), env = process.env}) {
  const verify = createConnectionVerifier(env);
  if (!verify) {
    if(env.INFUSER_INSTANCE_TOKEN || env.INFUSER_API_URL)throw new Error('Incomplete Infuser server configuration');
    return null;
  }
  if (!env.INFUSER_INSTANCE_TOKEN || !env.INFUSER_API_URL) throw new Error('Incomplete Infuser server credentials');
  const origin = new URL(env.INFUSER_API_URL);
  const dev = env.INFUSER_DEV === '1' && origin.protocol === 'http:' && ['127.0.0.1','localhost','[::1]'].includes(origin.hostname);
  if (!dev && (origin.protocol !== 'https:' || origin.origin !== env.INFUSER_CONNECTION_ISSUER)) throw new Error('Infuser API origin does not match its issuer');
  if (origin.pathname !== '/' || origin.search || origin.hash || origin.username || origin.password) throw new Error('Infuser API URL must be an origin');
  if (!sockets?.options?.noServer) throw new Error('Use a WebSocketServer with noServer:true');
  let registration, registering, coordinator, seq = 0, queue = Promise.resolve(), stopped = false;
  let controlState = 'starting', lastContact = Date.now();
  const reservations = new Map(); // launch/session -> held seat, including reconnect grace.
  const serial = fn => {const result = queue.then(fn); queue = result.catch(()=>{}); return result;};
  const seconds = () => Math.floor(Date.now()/1000);
  async function fetchJson(path, token, body, method) {
    const response = await fetch(new URL(path, origin), {method:method || (body === undefined ? 'GET' : 'POST'),
      redirect:'error', signal:AbortSignal.timeout(4000), headers:{Authorization:'Bearer '+token,
        ...(body === undefined ? {} : {'Content-Type':'application/json'})},
      ...(body === undefined ? {} : {body:JSON.stringify(body)})});
    const value = await response.json();
    if (!response.ok) {const error = new AdmissionError(value.error?.code || 'platform_unreachable', value.error?.message || (typeof value.error==='string'?value.error:'Infuser request failed')); error.status = response.status; throw error;}
    lastContact = Date.now();
    return value;
  }
  async function register(force = false) {
    if (!force && registration?.expires_at > seconds()+30) return registration;
    if (registering) return registering;
    registering = (async()=>{
      const result = await fetchJson('/api/server/v1/register',env.INFUSER_INSTANCE_TOKEN,{app_id:env.INFUSER_GAME_ID,instance_id:env.INFUSER_INSTANCE_ID});
      if (coordinator !== result.coordinator) {coordinator=result.coordinator;seq=0;}
      registration = result;
      if (controlState === 'starting') controlState = result.draining ? 'draining' : 'running';
      return result;
    })();
    try {return await registering;} finally {registering=undefined;}
  }
  async function api(path, body, method) {
    let auth = await register();
    try {return await fetchJson(path,auth.token,body,method);}
    catch(error) {
      if (error.status !== 401) throw error;
      auth = await register(true);
      return fetchJson(path,auth.token,body,method);
    }
  }
  const descriptor = value => ({id:value.id,label:value.label,activity:value.activity,joinable:value.joinable,max_players:value.max_players});
  async function release(entry, reason) {
    if (entry.released) return;
    entry.released = true; clearTimeout(entry.timer);
    reservations.delete(entry.key);
    if (entry.socket?.readyState < 2) entry.socket.close(1001,'Session ended');
    await entry.reservation.release?.(reason);
  }
  async function snapshot() {
    const rooms = new Map((sessions ? await sessions() : []).map(s => [s.id,{...descriptor(s),connections:[]}]));
    for (const entry of reservations.values()) {
      if (entry.released) continue;
      // An explicit game session list may end a run while its socket is open.
      if (sessions && !rooms.has(entry.reservation.session.id)) {await release(entry,'session_ended');continue;}
      const room = rooms.get(entry.reservation.session.id) || {...descriptor(entry.reservation.session),connections:[]};
      if (entry.socket?.readyState === 1) room.connections.push(entry.identity.connection_id);
      rooms.set(room.id,room);
    }
    const body = {coordinator,seq:++seq,sessions:[...rooms.values()]};
    try {
      const result = await api('/api/server/v1/sessions',body,'PUT');
      seq = Math.max(seq,result.seq);
    } catch(error) {
      if (error.code !== 'coordinator_restarted') throw error;
      await register(true);
      return snapshot();
    }
  }
  async function sync() {
    if (stopped) return;
    await register();
    const control = await api('/api/server/v1/control');
    if (control.coordinator !== coordinator) {await register(true);}
    if (control.state !== controlState) {controlState=control.state;await onControl(control);}
    if (controlState === 'stopping') {
      for (const entry of [...reservations.values()]) await release(entry,'server_stopping');
    }
    await snapshot();
  }
  const ready = serial(sync);
  ready.catch(onError);
  const timer = setInterval(()=>serial(sync).catch(onError),10000);
  timer.unref?.();
  const outage = setInterval(()=>{
    if (Date.now()-lastContact <= 45000) return;
    // Bound stale authority if the process can no longer contact its controller.
    for (const entry of reservations.values()) entry.socket?.terminate();
  },2000);
  outage.unref?.();
  function deny(raw, code) {
    if (raw.destroyed) return;
    raw.end('HTTP/1.1 409 Conflict\r\nConnection: close\r\nContent-Length: 0\r\n\r\n');
    const deadline=setTimeout(()=>raw.destroy(),1000);deadline.unref?.();
  }
  async function upgrade(request, raw, head) {
    let identity;
    try {identity=verify(request);} catch {deny(raw,'identity_expired');return;}
    let entry, attached = false, resumed = false, admissionTimer;
    const abort = new AbortController();
    const disconnected = () => abort.abort();
    raw.once('close',disconnected);
    try {
      await register();
      if (request.url !== registration.connection.path) throw new AdmissionError('admission_rejected');
      let intent = identity.request;
      if (!intent || !['play','create','join','resume'].includes(intent.action)) throw new AdmissionError('admission_rejected');
      if (controlState !== 'running' && intent.action !== 'resume') throw new AdmissionError('server_draining');
      if (Date.now()-lastContact > 45000) throw new AdmissionError('platform_unreachable');
      if (intent.action === 'play' && intent.activity === 'multiplayer') {
        const rooms = sessions ? await sessions() : [...reservations.values()].map(e=>e.reservation.session);
        const existing = rooms.filter(room=>room.activity==='multiplayer');
        if (existing.length) {
          const room = existing.find(room=>room.joinable);
          if (!room) throw new AdmissionError('session_unavailable');
          intent={...intent,action:'join',target:{session_id:room.id}};
        }
      }
      if (intent.action === 'resume') {
        entry = reservations.get(identity.launch_id+'/'+intent.target?.session_id);
        if (!entry || entry.released || (entry.disconnectedAt && Date.now()-entry.disconnectedAt > 30000)) throw new AdmissionError('resume_expired');
        resumed = true;
      } else {
        const pending = Promise.resolve(admit({identity, request:intent, signal:abort.signal}));
        // A late callback still owns its reservation until we explicitly release it.
        pending.then(value=>{if(abort.signal.aborted) Promise.resolve(value?.release?.('admission_cancelled')).catch(onError);}).catch(()=>{});
        const expired = new Promise((_,reject)=>{admissionTimer=setTimeout(()=>{abort.abort();reject(new AdmissionError('admission_rejected'));},4000);});
        const reservation = await Promise.race([pending,expired]);
        clearTimeout(admissionTimer);
        if (!reservation?.session || typeof reservation.attach !== 'function' || typeof reservation.release !== 'function') {
          await reservation?.release?.('invalid_reservation');throw new AdmissionError('admission_rejected');
        }
        if (abort.signal.aborted) throw new AdmissionError('admission_rejected');
        entry = {key:identity.launch_id+'/'+reservation.session.id,reservation,identity,released:false};
        if (reservations.has(entry.key)) {await reservation.release('already_connected');entry=undefined;throw new AdmissionError('already_connected');}
        if (intent.target && reservation.session.id !== intent.target.session_id) {await reservation.release('wrong_session');entry=undefined;throw new AdmissionError('session_ended');}
        reservations.set(entry.key,entry);
      }
      await api('/api/server/v1/connections/'+identity.connection_id+'/admission',{session:descriptor(entry.reservation.session)});
      if (abort.signal.aborted || raw.destroyed) throw new AdmissionError('admission_rejected');
      // handleUpgrade commits only after Infuser accepts the exact reserved room.
      sockets.handleUpgrade(request,raw,head,socket=>{
        const old = entry.socket;
        clearTimeout(entry.timer);entry.identity=identity;entry.disconnectedAt=0;entry.socket=socket;
        if (old && old !== socket) old.terminate();
        socket.once('close',code=>{
          if (entry.socket !== socket || entry.released) return;
          entry.socket=null;entry.disconnectedAt=Date.now();
          if(code===1000){serial(async()=>{await release(entry,'left_session');await snapshot();}).catch(onError);return;}
          Promise.resolve().then(()=>entry.reservation.suspend?.()).catch(onError);
          entry.timer=setTimeout(()=>serial(async()=>{await release(entry,'reconnect_expired');await snapshot();}).catch(onError),30000);
          entry.timer.unref?.();
          serial(snapshot).catch(onError);
        });
        socket.on('error',()=>{});
        attached=true;
        try {entry.reservation.attach(socket,{identity,resumed});} catch(error) {socket.terminate();throw error;}
      });
      await snapshot();
    } catch(error) {
      const code = codes.has(error.code) ? error.code : 'admission_rejected';
      try {await api('/api/server/v1/connections/'+identity.connection_id+'/admission',{error:code});} catch {}
      if (entry && !attached && !resumed) await release(entry,code);
      deny(raw,code);
      if (!(error instanceof AdmissionError)) onError(new AdmissionError('admission_rejected','Game admission failed'));
    } finally {clearTimeout(admissionTimer);raw.removeListener('close',disconnected);}
  }
  const handleUpgrade = (request,raw,head) => {
    raw.on('error',()=>{});
    // Serial admission avoids last-seat races, including asynchronous game callbacks.
    serial(()=>upgrade(request,raw,head)).catch(()=>raw.destroy());
  };
  server.on('upgrade',handleUpgrade);
  const apiObject = {
    ready,
    get draining() {return controlState !== 'running';},
    report: () => serial(snapshot),
    endSession: id => serial(async()=>{for(const entry of [...reservations.values()]) if(entry.reservation.session.id===id) await release(entry,'session_ended');await snapshot();}),
    close: async()=>{stopped=true;clearInterval(timer);clearInterval(outage);server.off('upgrade',handleUpgrade);await serial(async()=>{for(const entry of [...reservations.values()]) await release(entry,'server_stopping');});},
  };
  server.once('close',()=>apiObject.close().catch(onError));
  return apiObject;
}
