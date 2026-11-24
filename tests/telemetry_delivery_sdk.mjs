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
  recorder.emit('voice.stopped',{state:'pagehide'});
  assert.deepEqual(requests[1].body.events.map(e=>e.kind),['voice.stopped']);
  // Resolve the original request at a known RTT, then let the settled promises drain.
  time=1200;requests[0].finish(1600);
  await new Promise(setImmediate);
  time=1300;recorder.emit('page.exit');
  assert.equal(requests.at(-1).body.events.at(-1).at,1800);
  assert.equal(requests.at(-1).body.events.at(-1).data.uncertainty_ms,100);
  assert.ok(requests.every(r=>r.body.events.length));
  for(const req of requests.slice(1))req.finish(1800);
});
