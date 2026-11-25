import {sessionFetch} from './session-fetch.js';
import {ready, runtime, InfuserError} from '/_infuser/sdk/v1.js';

const base = '/_infuser/api/v1/connections/v1';
function failure(code, message, status = 409) {
  const error = new InfuserError(message || code, status);error.code=code;return error;
}
async function api(path, options={}) {
  const response=await sessionFetch(base+path,{credentials:'same-origin',...options});
  const value=await response.json();
  if(!response.ok) throw failure(value.error?.code || (response.status===401?'identity_expired':'platform_unreachable'),value.error?.message || (typeof value.error==='string'?value.error:'Infuser request failed'),response.status);
  return value;
}
const pause = (ms, signal) => new Promise((resolve,reject)=>{
  const abort=()=>{clearTimeout(timer);signal.removeEventListener('abort',abort);reject(signal.reason);};
  const timer=setTimeout(()=>{signal.removeEventListener('abort',abort);resolve();},ms);
  if(signal.aborted)abort();else signal.addEventListener('abort',abort,{once:true});
});

// Returns an ordinary WebSocket after exact room admission. Its infuser property
// carries the session/attempt IDs; gameplay messages have no platform envelope.
export async function connect(options={}) {
  const context=await ready;
  if(!context.capabilities.sdk?.capabilities?.includes('sessions.v1')) throw failure('unsupported_topology','This game needs dedicated sessions support.',422);
  const request=options.intent ? {action:'join',target:{session_id:options.intent.session_id,player_id:options.intent.player_id}}
    : {action:options.action || 'play',activity:options.activity ?? null,target:options.target ?? null};
  if(options.intent && options.intent.kind !== 'join_friend') throw failure('invalid_target','Unknown launch intent.',422);
  const abort=new AbortController();
  const deadline=setTimeout(()=>abort.abort(failure('connection_timeout','The game took too long to connect.',504)),Math.min(90000,Math.max(1000,options.timeoutMs || 60000)));
  const cancel=()=>abort.abort(failure('cancelled','Connection cancelled.',499));
  options.signal?.addEventListener('abort',cancel,{once:true});
  if(options.signal?.aborted)cancel();
  let socket,id,completed=false,buffering=true,buffer=[],bufferBytes=0,bufferError;
  const signal=abort.signal;
  try {
    signal.throwIfAborted();
    let server=await runtime.acquire({signal});
    while(server.state==='starting') {await pause(250,signal);server=await runtime.status({signal});}
    if(server.state!=='ready') throw failure('backend_unavailable',server.error || 'The game server is unavailable.',503);
    signal.throwIfAborted();
    const config=await api('',{signal});
    id=config.epoch+crypto.randomUUID().replaceAll('-','');
    const put=()=>api('/'+id,{method:'PUT',signal,headers:{'Content-Type':'application/json'},body:JSON.stringify(request)});
    try {await put();} catch(error) {
      // Only a lost response is retried. The exact ID and body recover the outcome.
      if(error.status || signal.aborted)throw error;
      await put();
    }
    const url=new URL(base+'/'+id+'/socket',location.origin);url.protocol='wss:';
    socket=new WebSocket(url,config.connection.protocols);
    socket.binaryType=options.binaryType || 'blob';
    const capture=event=>{
      if(!buffering)return;
      event.stopImmediatePropagation();
      const size=typeof event.data==='string'?new TextEncoder().encode(event.data).length:(event.data?.size ?? event.data?.byteLength ?? 0);
      bufferBytes+=size;
      if(buffer.length>=128 || bufferBytes>config.connection.max_message_bytes) {
        bufferError=failure('message_overflow','Too much game data arrived before admission.',413);socket.close(1009);return;
      }
      buffer.push(event.data);
    };
    socket.addEventListener('message',capture);
    await new Promise((resolve,reject)=>{
      const cleanup=()=>{socket.removeEventListener('open',open);socket.removeEventListener('error',failed);socket.removeEventListener('close',failed);signal.removeEventListener('abort',aborted);};
      const open=()=>{cleanup();resolve();};
      const failed=()=>{cleanup();reject(failure('connection_failed','Could not open the game connection.'));};
      const aborted=()=>{cleanup();socket.close();reject(signal.reason);};
      socket.addEventListener('open',open,{once:true});socket.addEventListener('error',failed,{once:true});socket.addEventListener('close',failed,{once:true});
      signal.addEventListener('abort',aborted,{once:true});if(signal.aborted)aborted();
    });
    let result;
    for (;;) {
      signal.throwIfAborted();
      if(bufferError)throw bufferError;
      result=await api('/'+id,{signal});
      if(result.state==='connected' && result.admitted)break;
      if(['failed','cancelled','closed'].includes(result.state))throw failure(result.error?.code || 'connection_lost','Could not join this session.');
      await pause(50,signal);
    }
    if(socket.readyState!==WebSocket.OPEN)throw failure('connection_lost','The connection closed during admission.');
    Object.defineProperty(socket,'infuser',{value:Object.freeze({attempt_id:id,session:result.session,lease_id:server.session_id})});
    completed=true;
    // Resolve first so game code can attach handlers, then replay early welcome
    // frames in order. Capture stays active until this flush, including new frames.
    setTimeout(()=>{buffering=false;socket.removeEventListener('message',capture);for(const data of buffer)socket.dispatchEvent(new MessageEvent('message',{data}));buffer=[];},0);
    return socket;
  } catch(error) {
    socket?.close();
    if(id && !signal.aborted) {
      // Browser handshake failures hide HTTP bodies. The owned attempt retains the
      // server's admission result, including full room or departed friend errors.
      try {
        for(let n=0;n<10;n++) {
          const result=await api('/'+id,{signal:AbortSignal.timeout(1500)});
          if(result.error?.code) {error=failure(result.error.code,result.error.code.replaceAll('_',' '));break;}
          if(['closed','cancelled'].includes(result.state))break;
          await pause(50,signal);
        }
      } catch {}
    }
    throw error;
  } finally {
    clearTimeout(deadline);options.signal?.removeEventListener('abort',cancel);
    if(!completed && id)api('/'+id,{method:'DELETE',signal:AbortSignal.timeout(2000)}).catch(()=>{});
  }
}

export async function disconnect(socket) {
  if(!socket?.infuser)throw new TypeError('An Infuser session socket is required');
  socket.close(1000,'Left session');
  await api('/'+socket.infuser.attempt_id,{method:'DELETE',signal:AbortSignal.timeout(5000)});
}
