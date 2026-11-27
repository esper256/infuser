import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFileSync} from 'node:fs';

const source = readFileSync(new URL('../web/telemetry.js',import.meta.url),'utf8').replace('export function recorder','function recorder');
test('first load and terminal events send immediately; acknowledged batches calibrate later events', async()=>{
  const requests=[];
  const window=new EventTarget(),document=new EventTarget();
  window.document=document;window.console={error(){}};
  document.querySelector=()=>null;
  const storage=()=>({setItem(k,v){this[k]=v;},getItem(k){return this[k];},removeItem(k){delete this[k];}});
  let time=1000;
  window.fetch=(_,opts)=>new Promise(resolve=>requests.push({body:JSON.parse(opts.body),finish:server=>resolve({ok:true,json:()=>({server_ms:server})})}));
  const start=vm.runInNewContext(source+';recorder',{window,document,crypto,performance,AbortSignal,queueMicrotask,
    navigator:{onLine:true,userAgent:'Firefox/120'},sessionStorage:storage(),localStorage:storage(),setInterval(){}});
  const recorder=start({scope:'example',source:'player',endpoint:'/events',now:()=>time});
  assert.deepEqual(requests[0].body.events.map(e=>e.kind),['page.load']);
  assert.equal(requests[0].body.events[0].at,1000);
  time=1100;
  document.dispatchEvent(new Event('freeze'));
  assert.deepEqual(requests[1].body.events.map(e=>e.kind),['page.freeze']);
  // Resolve the original request at a known RTT, then let the settled promises drain.
  time=1200;requests[0].finish(1600);
  await new Promise(setImmediate);
  time=1300;window.dispatchEvent(new Event('pagehide'));
  assert.equal(requests.at(-1).body.events.at(-1).at,1800);
  assert.equal(requests.at(-1).body.events.at(-1).data.uncertainty_ms,100);
  assert.ok(requests.every(r=>r.body.events.length));
  for(const req of requests.slice(1))req.finish(1800);
});
test('one multiplayer state per transition, one terminal operation, counted fetch failures',async()=>{
  const rows=[];let time=0,status=502,header=false;
  const make=vm.runInNewContext(source+';roots',{URL});
  const root=make((kind,data,src,at,seq)=>{
    const event={kind,data,src,at};
    if(seq!==undefined){rows[seq]=event;return seq;}
    rows.push(event);return rows.length-1;
  },()=>time,'library');
  for(let i=0;i<3600;i++){time+=1000;root.multiplayer('connected');root.drain();}
  assert.deepEqual(rows.map(e=>e.kind),['link']);
  root.multiplayer('connecting');time+=100;root.multiplayer('connected');root.drain(true);
  assert.equal(rows.at(-1).data.flaps,1);
  const op=root.op('voice.join');op.stage('grant');time+=12;op.stage('connect');time+=18;op.end('ok');op.end('fail');
  assert.deepEqual({...rows.at(-1).data.stages_ms},{grant:12,connect:18});
  const win={location:{href:'https://example.org/'},document:new EventTarget(),fetch:async()=>({status,ok:status===200,headers:{has:()=>header}})};
  root.observeWindow(win,'library');
  for(let i=0;i<4;i++){await win.fetch('/api/chat/SECRET?token=a#h');root.drain();}
  const failed=rows.filter(e=>e.kind==='fetch.fail');assert.equal(failed.length,1);
  assert.equal(failed[0].data.count,4);assert.equal(failed[0].data.route,'/api/chat/:id');
  header=true;await win.fetch('/api/voice');root.drain();assert.equal(rows.filter(e=>e.kind==='fetch.fail').length,1);
  const abandoned=root.op('mp.join');abandoned.stage('admission');root.exit();abandoned.end('fail');
  assert.equal(rows.filter(e=>e.kind==='op').length,2);
});

test('a game load measures observed engine work, first frame, resources and visible-page vitals',async()=>{
  const rows=[],frames=[];
  let time=0,finishEngine,observe;
  class Memory {buffer={byteLength:4096};}
  class Observer {
    static supportedEntryTypes=['longtask'];
    constructor(callback){observe=callback;}
    observe(){}
  }
  const memory=new Memory();
  const win={
    document:Object.assign(new EventTarget(),{getElementsByTagName:()=>({length:10})}),
    requestAnimationFrame:fn=>frames.push(fn),PerformanceObserver:Observer,
    WebAssembly:{Memory,instantiate:()=>new Promise(resolve=>{finishEngine=resolve;})},
    performance:{memory:{usedJSHeapSize:8192},getEntriesByType:()=>[
      {transferSize:150,decodedBodySize:200,duration:16},{transferSize:0,decodedBodySize:70,duration:8}
    ]}
  };
  const make=vm.runInNewContext(source+';roots',{URL});
  const root=make((kind,data)=>rows.push({kind,data}),()=>time,'player');
  root.observeWindow(win,'game');
  time=4;
  const loading=win.WebAssembly.instantiate();
  root.loaded();
  time=10;
  const instance={exports:{memory}};finishEngine({instance});
  assert.equal((await loading).instance,instance);
  time=12;
  for(const callback of frames.splice(0))callback(time);
  time=32;
  for(const callback of frames.splice(0))callback(time);
  observe({getEntries:()=>[{duration:60},{duration:75}]});
  root.vitals();
  assert.deepEqual(JSON.parse(JSON.stringify(rows)),[
    {kind:'op',data:{name:'game.load',outcome:'ok',stage:'first_frame',stages_ms:{frame:4,engine:6,first_frame:2},duration_ms:12,bytes:150,cached_ratio:50,largest_ms:16}},
    {kind:'page.vitals',data:{stalls:{count:2,max_ms:75},heap_bytes:8192,wasm_bytes:4096,dom_nodes:10,fps_low:50}}
  ]);
});

test('Library avoids the frame loop; only network and unhandled gateway failures count',async()=>{
  const rows=[],frames=[];
  let response;
  const root=vm.runInNewContext(source+';roots',{URL})((kind,data)=>rows.push({kind,data}),()=>0,'library');
  const win={location:{href:'https://example.org/'},document:new EventTarget(),requestAnimationFrame:fn=>frames.push(fn),
    fetch:async()=>{if(response instanceof Error)throw response;return response;}};
  root.observeWindow(win,'library');
  assert.equal(frames.length,0);
  for(const [status,handled] of [[404,false],[500,false],[502,false],[503,false],[504,false],[503,true]]){
    response={status,headers:{has:()=>handled}};await win.fetch('/asset.js');
  }
  for(const name of ['TypeError','AbortError']){
    response=Object.assign(new Error(),{name});await assert.rejects(win.fetch('/api/chat'),{name});
  }
  root.drain();
  assert.deepEqual(rows.map(r=>r.data.status??r.data.class),[502,503,504,'TypeError']);
});
test('error locations keep only code basenames and coordinates from browser frames',()=>{
  const locations=vm.runInNewContext(source+';errorLocation',{URL});
  const stack=['TypeError: secret at https://example.org/private.js:1:2',
    '    at run (https://192.0.2.1/path/bundle.min.js?token=private#secret:11:3)',
    'frame@https://example.org/hidden/core.wasm:19:4','    at data:text/javascript,secret:1:2',
    '    at https://example.org/a192.0.2.1.js:1:2','    at [native code]'].join('\n');
  assert.deepEqual(JSON.parse(JSON.stringify(locations('https://example.org/bundle.min.js?x=hidden',stack))),{
    file:'bundle.min.js',frames:['bundle.min.js:11:3','core.wasm:19:4']});
});

test('browser code basenames match the shared server fixtures',()=>{
  const codeFile=vm.runInNewContext(source+';codeFile',{URL});
  const cases=JSON.parse(readFileSync(new URL('./fixtures/diagnostic-basenames.json',import.meta.url)));
  for(const [name,valid] of cases)assert.equal(codeFile(name),valid?name:undefined,name);
});

test('repeated console strings and objects each capture one caller and fold into their first row',async()=>{
  let captures=0,tick;
  class TracedError extends Error {
    static captureStackTrace(caller){
      captures++;caller.stack='Error\n    at draw (https://example.org/game.js:12:3)';
    }
  }
  const window=new EventTarget(),document=new EventTarget(),requests=[];
  Object.assign(window,{document,Error:TracedError,console:{error(){}},fetch:async(_,opts)=>{
    requests.push(...JSON.parse(opts.body).events);
    return {ok:true,json:async()=>({server_ms:1000})};
  }});
  document.querySelector=()=>null;
  const storage=()=>({getItem(k){return this[k]},setItem(k,v){this[k]=v},removeItem(k){delete this[k]}});
  const start=vm.runInNewContext(source+';recorder',{window,document,URL,crypto,performance,Error:TracedError,
    navigator:{onLine:true,userAgent:'Firefox/120'},sessionStorage:storage(),localStorage:storage(),
    AbortSignal,queueMicrotask,setInterval(fn){tick=fn;}});
  start({scope:'fixture',source:'player',endpoint:'/events',now:()=>1000});
  for(let i=0;i<50;i++)window.console.error('repeated message');
  for(let i=0;i<20;i++)window.console.error({privateValue:i});
  await new Promise(setImmediate);
  tick();
  await new Promise(setImmediate);
  const rows=requests.filter(e=>e.kind==='console.error');
  assert.equal(captures,2);
  const latest=new Map(rows.map(e=>[e.seq,e.data]));
  assert.deepEqual([...latest.values()].map(d=>d.count).sort((a,b)=>a-b),[20,50]);
  for(const data of latest.values())assert.deepEqual(data.frames,['game.js:12:3']);
  assert.doesNotMatch(JSON.stringify(rows),/privateValue/);
});
