// Best-effort diagnostics. Recording, storage, and transport failures are isolated from play.
const prefix='infuser.diagnostics.v1.', ttl=48*3600000;
const wall=()=>performance.timeOrigin+performance.now();
const safe=fn=>(...args)=>{try{return fn(...args);}catch{/* Diagnostics are optional. */}};
const scrub=s=>typeof s==='string'?s.slice(0,1800)
  .replace(/\b(?:https?|wss?|file):\/\/[^\s)]+|[^\s]*[@?#=][^\s]*|\b(?:bearer|password|token|authorization|cookie|secret)\b\s*[:=]?\s*\S+|[A-Za-z0-9_-]{24,}|\b(?:\d{1,3}\.){3}\d{1,3}\b/gi,'[redacted]')
  .replace(/[\x00-\x1f\x7f]/g,' ').slice(0,280):'';
function fileName(url){try{return scrub(new URL(url,location.origin).pathname.split('/').pop());}catch{return '';}}
function frames(stack){
  if(typeof stack!=='string')return '';
  return stack.slice(0,4000).split('\n').flatMap(line=>{
    const match=line.match(/(?:https?:\/\/[^\s)]+\/)([^/?#:\s)]+)(?::(\d+))(?::(\d+))/);
    return match?[scrub(match[1])+':'+match[2]+':'+match[3]]:[];
  }).slice(0,10).join(' ');
}
function describe(error){return {
  message:scrub(typeof error==='string'?error:error?.message),
  class:scrub(error?.name),stack:frames(error?.stack)
};}
function stored(key){try{return JSON.parse(localStorage.getItem(key));}catch{return null;}}
export function recorder({source,endpoint,scope,headers={}}){
  if(!scope)return;
  const page=crypto.randomUUID(), key=prefix+page, transport=window.fetch.bind(window);
  const binding=JSON.stringify([endpoint,scope,headers]);
  let tab=page;
  safe(()=>{tab=sessionStorage.getItem(prefix+'tab')||page;sessionStorage.setItem(prefix+'tab',tab);})();
  let seq=0,queue=[],pending=false,flushAgain=false,offset=0,uncertainty,clockReady=false,clockAt=0,dropped=0;
  const seen=new Map(), attached=new WeakSet();
  const client=(navigator.userAgent.match(/(?:Firefox|Edg|Chrome|Version)\/\d+/)?.[0]||'browser')+' '+(navigator.userAgent.match(/Android|iPhone|iPad|Windows|CrOS|Macintosh|Linux/)?.[0]||'');
  const version=document.querySelector('meta[name="infuser-client-version"]')?.content;
  const keys=()=>Object.keys(localStorage).filter(k=>k.startsWith(prefix));
  const persist=safe(()=>{
    if(queue.length)localStorage.setItem(key,JSON.stringify({binding,expires:Date.now()+ttl,events:queue}));
    else localStorage.removeItem(key);
  });
  const sweep=safe(()=>{
    const live=[];
    for(const k of keys()){const rec=stored(k);if(!rec||rec.expires<Date.now())localStorage.removeItem(k);else live.push([rec.expires,k]);}
    live.sort((a,b)=>b[0]-a[0]).slice(12).forEach(([,k])=>localStorage.removeItem(k));
  });
  async function post(events){
    const res=await transport(endpoint,{method:'POST',credentials:'same-origin',keepalive:true,headers:{'Content-Type':'application/json',...headers},
      body:JSON.stringify({scope,events}),signal:AbortSignal.timeout(7000)});
    if(!res.ok)throw Error('diagnostic delivery failed');
    return (await res.json()).server_ms;
  }
  async function flush(){
    if(pending){flushAgain=true;return;}
    if(!navigator.onLine)return;
    pending=true;flushAgain=false;let delivered=false;
    try{
      sweep();
      if(!clockReady||Date.now()-clockAt>600000){
        const sent=wall(),server=await post([]),received=wall();
        if(!Number.isFinite(server))return;
        offset=server-(received+sent)/2;uncertainty=Math.ceil((received-sent)/2);clockReady=true;clockAt=Date.now();
      }
      const send=events=>post(events.map(({estimated,...e})=>({...e,at:estimated?e.at:Math.round(e.at+offset),data:{uncertainty_ms:uncertainty,...e.data}})));
      if(queue.length){
        const batch=queue.slice(0,8);
        await send(batch);
        const last=batch.at(-1).seq;queue=queue.filter(e=>e.seq>last);persist();
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
  const emit=safe((kind,data={},src=source)=>{
    if(queue.length>=24){
      const expendable=queue.findIndex(e=>['mem.sample','page.show','page.hide','resource.error','error.repeat'].includes(e.kind));
      queue.splice(Math.max(0,expendable),1);dropped++;
    }
    queue.push({page,seq:++seq,at:Math.round(wall()+(clockReady?offset:0)),estimated:clockReady,src,kind,data:{...data,tab,...(clockReady?{uncertainty_ms:uncertainty}:{})}});
    persist();
    if(['js.error','js.rejection','console.error','page.hide','page.freeze','page.exit','net.online'].includes(kind))void flush();
  });
  function error(kind,data,src){
    const fingerprint=[kind,data.class,data.message,data.file,data.line].join('|');
    const n=seen.get(fingerprint)||0;
    if(n>=3){seen.set(fingerprint,n+1);return;}
    if(seen.size>=64&&!seen.has(fingerprint)){dropped++;return;}
    seen.set(fingerprint,n+1);emit(kind,data,src);
  }
  const hook=safe((win,src)=>{
    const doc=win.document;if(attached.has(doc))return;attached.add(doc);
    win.addEventListener('error',safe(e=>{
      if(e.target!==win){error('resource.error',{file:fileName(e.target?.src||e.target?.href||''),class:scrub(e.target?.tagName)},src);return;}
      error('js.error',{...describe(e.error||e.message),file:fileName(e.filename),line:e.lineno,col:e.colno},src);
    }),true);
    win.addEventListener('unhandledrejection',safe(e=>error('js.rejection',describe(e.reason),src)));
    const original=win.console.error;
    win.console.error=function(...args){safe(()=>error('console.error',describe(args[0]),src))();return original.apply(this,args);};
    for(const [name,kind] of [['webglcontextlost','webgl.lost'],['webglcontextrestored','webgl.restored']])
      doc.addEventListener(name,()=>emit(kind,{},src),true);
  });
  sweep();hook(window,source);
  emit('page.load',{state:performance.getEntriesByType('navigation')[0]?.type,client,version});
  if(document.wasDiscarded)emit('page.discarded');
  document.addEventListener('visibilitychange',()=>emit(document.hidden?'page.hide':'page.show'));
  for(const name of ['freeze','resume'])document.addEventListener(name,()=>emit('page.'+name));
  window.addEventListener('pagehide',e=>emit('page.exit',{state:e.persisted?'cached':'departed'}));
  window.addEventListener('pageshow',e=>{if(e.persisted)emit('page.restore');});
  for(const name of ['online','offline'])window.addEventListener(name,()=>emit('net.'+name));
  window.addEventListener('infuser-voice-state',e=>{if(['connected','reconnecting','reconnected','disconnected','fail'].includes(e.detail?.state))emit('voice.'+e.detail.state,{state:scrub(String(e.detail.reason??'')),session:e.detail.session});});
  let sampleAt=0;
  setInterval(()=>{
    if(dropped){const count=dropped;dropped=0;emit('recorder.dropped',{count});}
    for(const [fp,n] of seen){if(n>3){emit('error.repeat',{message:scrub(fp),count:n-3});seen.set(fp,3);}}
    if(!document.hidden&&performance.now()-sampleAt>=300000&&performance.memory?.usedJSHeapSize){sampleAt=performance.now();emit('mem.sample',{heap_bytes:performance.memory.usedJSHeapSize,uptime_ms:Math.round(sampleAt)});}
    void flush();
  },15000);
  void flush();
  return {emit,hook};
}
