// Best-effort diagnostics. Recording, storage, and transport failures are isolated from play.
const prefix='infuser.diagnostics.v1.', ttl=48*3600000;
const wall=()=>performance.timeOrigin+performance.now();
const safe=fn=>(...args)=>{try{return fn(...args);}catch{/* Diagnostics are optional. */}};
// Unknown path components are opaque, even when they happen to look like ordinary words.
const routeParts=new Set('api _infuser v1 v2 voice control heartbeat profile appearance settings chat presence peers health context launch sessions saves cloud groups joinability ack input apps runtime signals ice player-pages close'.split(' '));
function routeOf(input,base){
  try{
    const url=new URL(typeof input==='string'?input:input.url??input,base);
    if(url.origin!==new URL(base).origin)return 'external';
    const parts=url.pathname.split('/').filter(Boolean);
    if(!['api','_infuser'].includes(parts[0]))return 'asset';
    return '/'+parts.slice(0,10).map(s=>routeParts.has(s)?s:':id').join('/');
  }catch{return 'unknown';}
}
const errorClasses=new Set(['Error','TypeError','SyntaxError','ReferenceError','RangeError','RuntimeError','AbortError','TimeoutError','NetworkError','NotAllowedError','NotFoundError','SecurityError','InvalidStateError','CompileError','LinkError']);
const errorClass=e=>errorClasses.has(e?.name)?e.name:'Error';

// Keep only code basenames. Never copy a raw stack line, even on a parse failure.
function codeFile(location){
  if(typeof location!=='string')return;
  let name=location;
  if(/^https?:\/\//i.test(location)){
    try{name=new URL(location).pathname.split('/').pop();}catch{return;}
  }
  if(name.length<=80 && /^[A-Za-z_][A-Za-z0-9_-]*(?:\.[A-Za-z0-9_-]+)*\.(?:[cm]?js|jsx|tsx?|wasm|html?)$/.test(name)
    && !/[a-f0-9]{32}|(?:[0-9]{1,3}\.){3}[0-9]{1,3}/i.test(name))return name;
}
function errorLocation(filename,stack,skip=0){
  const frames=[];
  let file=codeFile(filename);
  if(typeof stack==='string'){
    for(const line of stack.slice(0,8192).split('\n').slice(skip)){
      // V8 "at fn (url:l:c)" / "at url:l:c", or WebKit/Firefox "fn@url:l:c".
      const entry=/^\s*at (?:.* \()?((?:https?:\/\/|[A-Za-z_])[^()\s]*):(\d{1,9}):(\d{1,9})\)?$/.exec(line)
        || /^[^@\s]*@((?:https?:\/\/|[A-Za-z_])\S*):(\d{1,9}):(\d{1,9})$/.exec(line);
      if(!entry)continue;
      const name=codeFile(entry[1]);
      if(!name)continue;
      file??=name;
      frames.push(name+':'+entry[2]+':'+entry[3]);
      if(frames.length===5)break;
    }
  }
  return {...(file?{file}:{}),...(frames.length?{frames}:{})};
}

function roots(emit,now,source){
  const links=new Map(),open=new Set();
  let failure,room,mpJoin,gameLoad,gameWin,enginePending=0,frameLoaded=false,frameRequested=false;
  let stallCount=0,stallMax=0,slowestFrame=0,prevFrame,wasmMemory;
  function drainFailure(){
    if(failure&&failure.sent!==failure.data.count){failure.seq=emit('fetch.fail',{...failure.data},failure.src,failure.at,failure.seq);failure.sent=failure.data.count;}
  }
  function failed(src,route,method,status,exception,started){
    const data={route,method,status,class:exception,duration_ms:Math.max(0,Math.round(now()-started)),count:1};
    const key=[src,route,method,status,exception].join('|');
    if(failure?.key===key){failure.data.count++;failure.data.duration_ms+=data.duration_ms;}
    else{drainFailure();failure={key,data,src,at:now()};}
  }
  function commit(l){
    if(l.pending){emit('link',l.pending,source,l.since);l.pending=null;}
  }
  function link(name){
    if(!links.has(name)){
      const l={state:'',since:now()};
      l.set=(state,reason,refs={})=>{
        if(state===l.state)return;
        const at=now(),prev=l.state,held=Math.max(0,Math.round(at-l.since));
        if(l.pending && state===l.pending.prev && held<5000){
          l.pending.flaps++;l.pending.state=state;l.pending.down_ms+=['down','degraded'].includes(prev)?held:0;
          if(['down','degraded'].includes(prev))l.pending.down_reason=l.reason;
          l.pending.reason=reason;l.pending.held_ms+=held;Object.assign(l.pending,refs);
        }else{
          commit(l);l.pending={name,state,prev:prev||'none',reason,held_ms:prev?held:0,flaps:0,down_ms:['down','degraded'].includes(prev)?held:0,down_reason:l.reason,...refs};
        }
        l.state=state;l.since=at;l.reason=reason;
      };
      links.set(name,l);
    }
    return links.get(name);
  }
  function op(name,refs={}){
    const begin=now(),stages={};let step,started=begin,done=false;
    function finishStage(at){if(step)stages[step]=(stages[step]||0)+Math.max(0,Math.round(at-started));}
    const operation={
      stage(s){if(done||step===s)return;const at=now();finishStage(at);step=s;started=at;},
      end(outcome,reason,extra={}){if(done)return;done=true;open.delete(operation);const at=now();finishStage(at);
        if(reason?.startsWith('error:')&&!errorClasses.has(reason.slice(6)))reason='error:Error';
        emit('op',{name,outcome,reason,stage:step,stages_ms:stages,duration_ms:Math.max(0,Math.round(at-begin)),...refs,...extra});}
    };
    open.add(operation);return operation;
  }
  if(source==='player'){gameLoad=op('game.load');gameLoad.stage('frame');}
  function firstFrame(){
    if(!gameLoad||!frameLoaded||enginePending||frameRequested||!gameWin)return;
    frameRequested=true;gameLoad.stage('first_frame');
    gameWin.requestAnimationFrame(safe(()=>{
      frameRequested=false;if(enginePending||!gameLoad)return;
      const timings=gameWin.performance.getEntriesByType('resource');
      let bytes=0,cached=0,largest=0;
      for(const r of timings){bytes+=r.transferSize||0;cached+=Number(r.transferSize===0&&r.decodedBodySize>0);largest=Math.max(largest,r.duration||0);}
      gameLoad.end('ok',undefined,{bytes,cached_ratio:timings.length?Math.round(cached*100/timings.length):0,largest_ms:Math.round(largest)});
      gameLoad=null;
    }));
  }
  function observeWindow(win,src){
    if(src==='game'){
      gameWin=win;
      for(const method of ['instantiate','instantiateStreaming']){
        const original=win.WebAssembly?.[method];if(!original)continue;
        win.WebAssembly[method]=function(...args){
          if(!gameLoad)return original.apply(this,args);
          gameLoad.stage('engine');enginePending++;
          function complete(value){
            enginePending--;
            safe(()=>{
              const exports=(value?.instance??value)?.exports;
              const memory=exports&&Object.values(exports).find(v=>v instanceof win.WebAssembly.Memory);
              if(memory&&typeof WeakRef==='function')wasmMemory=new WeakRef(memory);
              firstFrame();
            })();
            return value;
          }
          function rejected(e){
            enginePending--;
            gameLoad?.end('fail','error:'+errorClass(e));gameLoad=null;
            throw e;
          }
          try{return original.apply(this,args).then(complete,rejected);}
          catch(e){return rejected(e);}
        };
      }
    }
    if(src==='game'||source==='library'){
      if(win.PerformanceObserver?.supportedEntryTypes?.includes('longtask')){
        new win.PerformanceObserver(list=>{for(const item of list.getEntries()){stallCount++;stallMax=Math.max(stallMax,item.duration);}}).observe({type:'longtask'});
      }
    }
    if(src==='game'){
      const frame=t=>{
        if(!win.document.hidden && prevFrame!==undefined)slowestFrame=Math.max(slowestFrame,t-prevFrame);
        prevFrame=win.document.hidden?undefined:t;win.requestAnimationFrame(frame);
      };
      win.requestAnimationFrame?.(frame);
      win.document.addEventListener('visibilitychange',()=>{prevFrame=undefined;});
    }
    const original=win.fetch;
    if(!original)return;
    win.fetch=async function(input,options){
      const started=now(),route=routeOf(input,win.location?.href),verb=String(options?.method??input?.method??'GET').toUpperCase();
      const method=['GET','POST','PUT','PATCH','DELETE','HEAD','OPTIONS'].includes(verb)?verb:'OTHER';
      const joining=src==='game' && method==='POST' && /^\/_infuser\/(?:api\/v1\/)?peers$/.test(route);
      let joiningOp;
      if(joining){mpJoin?.end('abandoned','user');room=undefined;joiningOp=mpJoin=op('mp.join');joiningOp.stage('admission');}
      let response;
      try{response=await original.apply(this,arguments);}
      catch(e){safe(()=>{if(['TypeError','NetworkError','TimeoutError'].includes(e?.name))failed(src,route,method,undefined,errorClass(e),started);joiningOp?.end('fail','error:'+errorClass(e));})();throw e;}
      safe(()=>{
        if([502,503,504].includes(response.status) && !response.headers.has('x-infuser-request-id'))failed(src,route,method,response.status,undefined,started);
        else if(failure?.src===src && failure.data.route===route && failure.data.method===method){drainFailure();failure=null;}
        if(joiningOp && joiningOp===mpJoin){
          if(!response.ok)joiningOp.end('fail','error');
          else{joiningOp.stage('connect');void response.clone().json().then(body=>{if(joiningOp===mpJoin && /^[0-9a-f]{32}$/.test(body.epoch))room=body.epoch;}).catch(()=>{});}
        }
      })();
      return response;
    };
  }
  return {link,op,observeWindow,
    multiplayer(status){const state={ended:'ended',connecting:'connecting',connected:'up'}[status];if(!state)return;
      const refs=room?{room}:{};
      link('mp').set(state,undefined,refs);
      if(state==='up'||state==='ended'){mpJoin?.end(state==='up'?'ok':'fail',undefined,refs);}
    },
    loaded(){frameLoaded=true;firstFrame();},
    loadFailed(e){gameLoad?.end('fail','error:'+errorClass(e));link('launch').set('ended','error:'+errorClass(e));},
    drain(terminal=false){drainFailure();for(const l of links.values())if(terminal||now()-l.since>=5000)commit(l);},
    exit(){for(const o of open)o.end('abandoned','page_exit');this.drain(true);
      return [...links].filter(([,l])=>l.state==='up'||l.state==='degraded').map(([name])=>name);},
    vitals(){
      const win=gameWin||window, data={stalls:{count:stallCount,max_ms:Math.round(stallMax)}};
      const heap=win.performance?.memory?.usedJSHeapSize;if(heap!==undefined)data.heap_bytes=heap;
      const wasm=wasmMemory?.deref()?.buffer?.byteLength??win.Module?.HEAPU8?.buffer?.byteLength;if(wasm!==undefined)data.wasm_bytes=wasm;
      data.dom_nodes=win.document.getElementsByTagName('*').length;
      if(slowestFrame>0)data.fps_low=Math.round(1000/slowestFrame);
      stallCount=stallMax=slowestFrame=0;emit('page.vitals',data);
    }
  };
}
function stored(key){try{return JSON.parse(localStorage.getItem(key));}catch{return null;}}
export function recorder({source,endpoint,scope,headers={},appearance,now=wall}){
  if(!scope)return;
  const page=crypto.randomUUID(), key=prefix+page, transport=window.fetch.bind(window);
  const binding=JSON.stringify([endpoint,scope,headers]);
  let tab=page;
  safe(()=>{tab=sessionStorage.getItem(prefix+'tab')||page;sessionStorage.setItem(prefix+'tab',tab);})();
  let seq=0,queue=[],pending=false,flushAgain=false,offset=0,uncertainty,clockReady=false,dropped=0;
  const clockKey='infuser.diagnosticClock.v1';
  const cached=stored(clockKey);
  let clockSent=0;
  if(cached&&Number.isFinite(cached.offset)&&Number.isFinite(cached.uncertainty)&&cached.at<=now()&&now()-cached.at<600000){
    offset=cached.offset;uncertainty=cached.uncertainty;clockReady=true;clockSent=cached.at;
  }
  const seen=new Map(), attached=new WeakSet();
  const client=(navigator.userAgent.match(/(?:Firefox|Edg|Chrome|Version)\/\d+/)?.[0]||'browser')+' '+(navigator.userAgent.match(/Android|iPhone|iPad|Windows|CrOS|Macintosh|Linux/)?.[0]||'');
  const version=document.querySelector('meta[name="infuser-client-version"]')?.content;
  const keys=()=>Object.keys(localStorage).filter(k=>k.startsWith(prefix));
  const persist=safe(()=>{
    if(queue.length)localStorage.setItem(key,JSON.stringify({binding,expires:now()+ttl,events:queue}));
    else localStorage.removeItem(key);
  });
  const sweep=safe(()=>{
    const live=[];
    for(const k of keys()){const rec=stored(k);if(!rec||rec.expires<now())localStorage.removeItem(k);else live.push([rec.expires,k]);}
    live.sort((a,b)=>b[0]-a[0]).slice(12).forEach(([,k])=>localStorage.removeItem(k));
  });
  async function post(events,clientMs){
    const res=await transport(endpoint,{method:'POST',credentials:'same-origin',keepalive:true,headers:{'Content-Type':'application/json',...headers},
      body:JSON.stringify({scope,events,client_ms:Math.round(clientMs)}),signal:AbortSignal.timeout(7000)});
    if(!res.ok)throw Error('diagnostic delivery failed');
    return (await res.json()).server_ms;
  }
  async function send(events){
    const sent=now();
    const server=await post(events.map(({estimated,...e})=>({...e,at:estimated?e.at:Math.round(e.at+offset),data:{uncertainty_ms:uncertainty,...e.data}})),sent);
    const received=now();
    if(Number.isFinite(server)&&sent>=clockSent){
      offset=server-(received+sent)/2;
      uncertainty=Math.ceil((received-sent)/2);
      clockReady=true;clockSent=sent;
      safe(()=>localStorage.setItem(clockKey,JSON.stringify({offset,uncertainty,at:sent})))();
    }
  }
  const inFlight=new Set();
  async function deliver(batch){
    for(const e of batch)inFlight.add(e.seq);
    try{
      await send(batch);
      queue=queue.filter(e=>!batch.includes(e));persist();
    }finally{for(const e of batch)inFlight.delete(e.seq);}
  }
  async function flush(lastChance=false){
    signals.drain(lastChance);
    for(const group of seen.values()){
      if(group.sent===group.count)continue;
      group.sent=group.count;
      emit(group.kind,{...group.data,count:group.count},group.src,group.at,group.seq);
    }
    if(pending){
      flushAgain=true;
      if(lastChance){
        const batch=queue.filter(e=>!inFlight.has(e.seq)).slice(-8);
        if(batch.length)await deliver(batch).catch(()=>{});
      }
      return;
    }
    if(!navigator.onLine)return;
    pending=true;flushAgain=false;let delivered=false;
    try{
      sweep();
      if(queue.length){
        const available=queue.filter(e=>!inFlight.has(e.seq));
        const batch=lastChance?available.slice(-8):available.slice(0,8);
        if(batch.length)await deliver(batch);
      }
      // Other pages may still be writing. Never overwrite a record changed during delivery.
      for(const k of safe(keys)()||[]){
        if(k===key)continue;
        const raw=localStorage.getItem(k),old=stored(k);
        if(old?.binding!==binding||!Array.isArray(old.events)||!old.events.length)continue;
        for(let i=0;i<old.events.length;i+=8)await send(old.events.slice(i,i+8));
        if(localStorage.getItem(k)===raw)localStorage.removeItem(k);
        break;
      }
      delivered=true;
    }catch{/* Keep the bounded outbox for the next flush. */}finally{
      pending=false;
      if(delivered&&flushAgain&&queue.length)queueMicrotask(flush);
    }
  }
  const emit=safe((kind,data={},src=source,at=now(),priorSeq)=>{
    if(priorSeq!==undefined)queue=queue.filter(e=>e.seq!==priorSeq);
    if(queue.length>=24){
      const expendable=queue.findIndex(e=>['page.vitals','page.show','page.hide','resource.error','error.repeat'].includes(e.kind));
      queue.splice(Math.max(0,expendable),1);dropped++;
    }
    const sequence=priorSeq??++seq;
    queue.push({page,seq:sequence,at:Math.round(at+(clockReady?offset:0)),estimated:clockReady,src,kind,data:{...data,tab,...(clockReady?{uncertainty_ms:uncertainty}:{})}});
    persist();
    if(priorSeq===undefined&&['js.error','js.rejection','console.error','page.hide','page.freeze','page.exit','net.online'].includes(kind))void flush(['page.freeze','page.exit'].includes(kind));
    return sequence;
  });
  function error(kind,data,src,detail){
    const signature=[kind,data.class,data.file,data.line,data.col,data.frames?.[0],typeof detail==='string'?detail.split(/[?#]/,1)[0].slice(0,250):''].join('|');
    let hash=2166136261;
    for(let i=0;i<signature.length;i++)hash=Math.imul(hash^signature.charCodeAt(i),16777619);
    const fingerprint=(hash>>>0).toString(16).padStart(8,'0');
    data.fingerprint=fingerprint;
    const key=src+'|'+fingerprint;
    const group=seen.get(key);
    if(group){group.count++;return;}
    if(seen.size>=64){dropped++;return;}
    const at=now();
    const seq=emit(kind,{...data,count:1},src,at);
    seen.set(key,{kind,data,src,at,seq,count:1,sent:1});
  }
  const hook=safe((win,src)=>{
    const doc=win.document;if(attached.has(doc))return;attached.add(doc);
    signals.observeWindow(win,src);
    win.addEventListener('error',safe(e=>{
      if(e.target!==win){error('resource.error',{class:'ResourceError'},src,e.target?.src||e.target?.href);return;}
      error('js.error',{class:errorClass(e.error),line:e.lineno,col:e.colno,...errorLocation(e.filename,e.error?.stack)},src,e.message);
    }),true);
    win.addEventListener('unhandledrejection',safe(e=>error('js.rejection',{class:errorClass(e.reason),...errorLocation(undefined,e.reason?.stack)},src,e.reason?.message)));
    const original=win.console.error;
    win.console.error=function diagnosticConsoleError(...args){
      const caller=new Error();
      safe(()=>{
        const err=args.find(arg=>arg instanceof win.Error);
        // Safari/Firefox start at the wrapper; V8 also has an Error header.
        const skip=caller.stack?.startsWith('Error\n')?2:1;
        error('console.error',{class:errorClass(err),...errorLocation(undefined,err?.stack??caller.stack,err?.stack?0:skip)},src,err?.message??args[0]);
      })();
      return original.apply(this,args);
    };
    for(const [name,kind] of [['webglcontextlost','webgl.lost'],['webglcontextrestored','webgl.restored']])
      doc.addEventListener(name,()=>emit(kind,{},src),true);
  });
  const signals=roots(emit,now,source);
  sweep();hook(window,source);
  const updating=safe(()=>{const v=sessionStorage.getItem(prefix+'update');sessionStorage.removeItem(prefix+'update');return v;})();
  const cause=updating?'reload_update':document.wasDiscarded?'discarded':performance.getEntriesByType('navigation')[0]?.type==='reload'?'reload':'navigate';
  emit('page.load',{cause,appearance,client,version});
  if(document.wasDiscarded)emit('page.discarded');
  document.addEventListener('visibilitychange',()=>emit(document.hidden?'page.hide':'page.show'));
  for(const name of ['freeze','resume'])document.addEventListener(name,()=>emit('page.'+name));
  window.addEventListener('pagehide',e=>emit('page.exit',{state:e.persisted?'cached':'departed',links:signals.exit()}));
  window.addEventListener('pageshow',e=>{if(e.persisted)emit('page.restore');});
  for(const name of ['online','offline'])window.addEventListener(name,()=>emit('net.'+name));
  let sampleAt=now();
  setInterval(()=>{
    if(dropped){const count=dropped;dropped=0;emit('recorder.dropped',{count});}
    if(!document.hidden&&now()-sampleAt>=300000){sampleAt=now();safe(()=>signals.vitals())();}
    void flush();
  },15000);
  void flush();
  return {hook,...signals,markUpdate:safe(()=>sessionStorage.setItem(prefix+'update','1'))};
}
