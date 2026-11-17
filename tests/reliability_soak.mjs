// Opt-in Linux process-crash journey. Owns only its disposable fixture process group.
// Build dedicated_fixture and stage dedicated-node first; see developer-testing.md.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {setTimeout as sleep} from 'node:timers/promises';
import {randomUUID} from 'node:crypto';
import http from 'node:http';
import {startProxy,chromium,browserOptions} from './support/dedicated_browser.mjs';

assert.equal(process.platform,'linux','This harness uses Linux process groups and /proc.');
const duration=Number(process.env.INFUSER_SOAK_SECONDS || 7200)*1000;
const interval=Number(process.env.INFUSER_SOAK_INTERVAL_SECONDS || 120)*1000;
const faults=(process.env.INFUSER_SOAK_FAULTS || 'transport,background,backend,controller').split(',');
assert(Number.isFinite(duration) && duration>=60000 && duration<=86400000);
assert(Number.isFinite(interval) && interval>=0 && interval<=600000);
assert(faults.length && faults.every(fault=>['transport','background','backend','controller'].includes(fault)));
const root=fs.mkdtempSync(path.join(os.tmpdir(),'infuser-reliability-'));
fs.chmodSync(root,0o700);
const ready=path.join(root,'ready.json'),state=path.join(root,'state');
const log=fs.openSync(path.join(root,'fixture.log'),'a',0o600);
const executable=path.resolve(process.env.INFUSER_FIXTURE_BIN || 'target/debug/examples/dedicated_fixture');
const project=path.resolve(process.argv[2] || '.local/dedicated-sdk-project');
let child,config,proxy,browser,origin,cycles=0,complete=false;
let cleaning;
const started=Date.now();
const evidence=(event,fields={})=>console.log(JSON.stringify({time:new Date().toISOString(),elapsed_seconds:Math.round((Date.now()-started)/1000),event,...fields}));
// A stuck API request must fail the run, not keep a developer machine busy forever.
const watchdog=setTimeout(()=>{
  evidence('timed_out');void cleanup().finally(()=>process.exit(1));
},duration+300000);
watchdog.unref();
function cleanup() {
  return cleaning ??= (async()=>{
    clearTimeout(watchdog);
    if(child?.pid){try{process.kill(-child.pid,'SIGKILL');}catch(error){if(error.code!=='ESRCH')throw error;}}
    if(browser)await browser.close();
    if(proxy){proxy.closeAllConnections();await new Promise(resolve=>proxy.close(resolve));}
    fs.closeSync(log);
    if(complete && !process.env.INFUSER_KEEP_SOAK_STATE)fs.rmSync(root,{recursive:true,force:true});
    else evidence('artifacts',{directory:root});
  })();
}
for(const [signal,code] of [['SIGINT',130],['SIGTERM',143]])process.once(signal,()=>{
  evidence('interrupted',{signal});void cleanup().finally(()=>process.exit(code));
});
async function until(check,label,timeout=30000) {
  const deadline=Date.now()+timeout;
  while(Date.now()<deadline){if(await check())return;await sleep(200);}
  throw Error('Timed out: '+label);
}
async function start() {
  child=spawn(executable,[project,ready,state],{detached:true,stdio:['ignore',log,log],env:{...process.env,INFUSER_TEST_NODE:process.execPath}});
  let failure;child.on('error',error=>{failure=error;});
  await until(async()=>{
    if(failure)throw failure;
    assert.equal(child.exitCode,null,'Fixture exited; inspect '+root+'/fixture.log');
    if(!fs.existsSync(ready))return false;
    const next=JSON.parse(fs.readFileSync(ready));
    if(next.pid!==child.pid)return false;
    try {if(!(await fetch(next.api+'/api/library',{headers:{Cookie:'member=river'}})).ok)return false;}catch{return false;}
    config=next;return true;
  },'controller ready');
}
async function crashController() {
  const exited=new Promise(resolve=>child.once('exit',resolve));
  // SIGKILL bypasses destructors, graceful HTTP shutdown and database cleanup.
  // Kill the fixture's own backend too: this models a host/process-group crash.
  process.kill(-child.pid,'SIGKILL');await exited;
  await start();
}
function crashBackend() {
  const pid=Number(fs.readFileSync(path.join(state,'backend.pid'),'utf8'));
  const stat=fs.readFileSync(`/proc/${pid}/stat`,'utf8').split(') ')[1].split(' ');
  assert.equal(Number(stat[2]),child.pid,'Refuse to kill a process outside our fixture group');
  process.kill(pid,'SIGKILL');
}
async function member(name,guest=false) {
  const context=await browser.newContext({ignoreHTTPSErrors:true});
  await context.addCookies([{name:'member',value:name,url:origin}]);
  const main=await context.newPage();await main.goto(origin+'/library');
  const result=await main.evaluate(async guest=>{
    const r=await fetch('/api/apps/dedicated-lab/launch',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({guest})});
    return {status:r.status,body:await r.json()};
  },guest);
  assert.equal(result.status,200,JSON.stringify(result.body));
  const page=await context.newPage();await page.goto(result.body.url);
  const frame=await (await page.locator('#game').elementHandle()).contentFrame();
  await frame.waitForFunction(()=>!!window.fixture);
  return {context,page,frame};
}
async function play(game,options={activity:'multiplayer'}) {
  await game.frame.evaluate(options=>fixture.play(options),options);
  try {await game.frame.waitForFunction(()=>fixture.socket?.readyState===1 && fixture.state.players.some(p=>p.id===fixture.id),null,{timeout:20000});}
  catch(error){throw Error('Admission: '+await game.frame.locator('#status').innerText(),{cause:error});}
  return game.frame.evaluate(()=>({player:fixture.id,session:fixture.socket.infuser.session.id}));
}
const load=game=>game.frame.evaluate(()=>fixture.platform.cloud.load('campaign'));
const commit=(game,checkpoint)=>game.frame.evaluate(checkpoint=>fixture.platform.cloud.commit('campaign',checkpoint),checkpoint);
const packet=(revision,level)=>({commitId:randomUUID(),expectedRevision:revision,files:{'progress.json':{level}},summary:`Checkpoint ${level}`});
async function publish(route,method='GET',body) {
  const raw=Buffer.isBuffer(body);
  const response=await fetch(config.api+'/api/publisher/apps/dedicated-lab'+route,{method,headers:{Authorization:'Bearer '+config.token,...(body===undefined?{}:{'Content-Type':raw?'application/octet-stream':'application/json'})},body:body===undefined?undefined:raw?body:JSON.stringify(body)});
  const result=await response.json();assert(response.ok,JSON.stringify(result));return result;
}
const uploadBytes=Buffer.alloc(4096,0x5a);
async function partialUpload() {
  const request=http.request(config.api+'/api/publisher/apps/dedicated-lab/builds/recovery/files/probe.bin',{method:'PUT',headers:{Authorization:'Bearer '+config.token,'Content-Type':'application/octet-stream','Content-Length':8*1024*1024}});
  request.on('error',()=>{});request.on('response',r=>r.resume());request.write(Buffer.alloc(1024*1024,0x11));
  await until(async()=>fs.readdirSync(path.join(state,'upload-staging')).some(name=>fs.statSync(path.join(state,'upload-staging',name)).size>=1024*1024),'partial upload reached disk');
  return request;
}
async function verifyUploadRecovery() {
  await until(async()=>!fs.existsSync(path.join(state,'upload-staging')) || fs.readdirSync(path.join(state,'upload-staging')).length===0,'interrupted upload cleaned up');
  assert.deepEqual(fs.readFileSync(path.join(state,'content/dedicated-lab/recovery/probe.bin')),uploadBytes,'An incomplete replacement must preserve the previous artifact');
  assert.equal((await publish('/builds/recovery')).files['probe.bin'],uploadBytes.length);
  await publish('/builds/recovery/files/probe.bin','PUT',uploadBytes);
  assert.equal((await publish('/builds/one')).publication_state,'published','Unfinished uploads must not replace the playing build');
}
async function moving(a,b) {
  const before=await a.frame.evaluate(()=>fixture.state.tick);
  await a.frame.waitForFunction(tick=>fixture.state.tick!==tick && fixture.state.players.length===2,before);
  await b.frame.waitForFunction(()=>fixture.state.players.length===2);
  assert.equal(await a.frame.evaluate(()=>fixture.socket.infuser.session.id),await b.frame.evaluate(()=>fixture.socket.infuser.session.id));
}
function resources() {
  const status=fs.readFileSync(`/proc/${child.pid}/status`,'utf8');
  return {controller_rss_kib:Number(status.match(/^VmRSS:\s+(\d+)/m)?.[1]),controller_fds:fs.readdirSync(`/proc/${child.pid}/fd`).length,database_bytes:fs.statSync(path.join(state,'platform.sqlite')).size};
}
try {
  await start();proxy=await startProxy(config);origin=`https://games.example.com:${proxy.address().port}`;
  browser=await chromium.launch(browserOptions);
  const river=await member('river'),sky=await member('sky'),guest=await member('river',true);
  let room=await play(river);await play(sky,{action:'join',target:{session_id:room.session}});
  await moving(river,sky);
  let revision=null;
  // Lose an acknowledged write's HTTP response. The SDK must retry the same ID.
  const initial=packet(null,0);let dropped=0;
  await river.page.route('**/_infuser/api/v1/cloud/v1/groups/campaign',async route=>{
    if(route.request().method()==='POST' && !dropped){
      // APIRequestContext does not inherit Chromium's test DNS resolver rules.
      const url=new URL(route.request().url());const host=url.host;url.hostname='127.0.0.1';
      const response=await route.fetch({url:url.href,headers:{...route.request().headers(),host}});
      assert.equal(response.status(),200);dropped++;
      await response.dispose();await route.abort('connectionreset');
    }else await route.continue();
  });
  const first=await commit(river,initial);assert.equal(dropped,1);revision=first.revision;
  await river.page.unroute('**/_infuser/api/v1/cloud/v1/groups/campaign');
  assert.equal((await commit(river,initial)).revision,revision);
  assert.equal((await load(sky)).revision,null);
  assert.equal((await load(guest)).revision,null);
  const guestWrite=await commit(guest,packet(null,-1));
  assert.equal((await load(river)).revision,revision);
  const original=await publish('/builds/one');
  await publish('/builds/recovery','PUT',{manifest:original.manifest,server:original.server});
  await publish('/builds/recovery/files/probe.bin','PUT',uploadBytes);
  evidence('started',{duration_seconds:duration/1000,interval_seconds:interval/1000,faults,...resources()});
  while(Date.now()-started<duration || cycles<faults.length) {
    const cycleStarted=Date.now();cycles++;
    const checkpoint=packet(revision,cycles);
    const saved=await commit(river,checkpoint);assert.equal(saved.status,'saved');revision=saved.revision;
    const fault=faults[(cycles-1)%faults.length];
    if(fault==='transport') {
      const upload=await partialUpload();upload.destroy();await verifyUploadRecovery();
      const original=room;
      await river.frame.evaluate(()=>fixture.socket.close());
      await river.frame.waitForFunction(()=>!fixture.socket);
      room=await play(river,{action:'resume',target:{session_id:original.session}});
      assert.deepEqual(room,original);
    } else if(fault==='background') {
      const cdp=await river.context.newCDPSession(river.page);
      await cdp.send('Page.setWebLifecycleState',{state:'frozen'});
      await sleep(35000); // More than the startup lease, using real elapsed time.
      await cdp.send('Page.setWebLifecycleState',{state:'active'});await cdp.detach();
      assert.equal(await river.frame.evaluate(()=>fixture.socket?.readyState),1);
    } else {
      const oldBackend=fs.readFileSync(path.join(state,'backend.pid'),'utf8');
      if(fault==='backend')crashBackend();else {
        const upload=await partialUpload();await crashController();upload.destroy();
        await verifyUploadRecovery();
      }
      await Promise.all([river,sky].map(game=>game.frame.waitForFunction(()=>!fixture.socket)));
      // The fixture keeps rooms in RAM. A dead room must not silently resume
      // as another run, even though durable cloud saves are still available.
      await until(async()=>{
        const response=await fetch(config.api+`/api/publisher/apps/dedicated-lab/builds/one`,{headers:{Authorization:'Bearer '+config.token}});
        return response.ok;
      },'publisher credential survives restart');
      // Production deliberately backs off failed servers for 30 seconds. Keep
      // that real delay: callers may retry, but must not spin up a crash loop.
      await until(async()=>{
        const result=await river.frame.evaluate(async()=>{
          try{return await fixture.platform.runtime.acquire();}
          catch(error){return {status:error.status,message:error.message};}
        });
        assert(!result.status || [409,503].includes(result.status),JSON.stringify(result));
        return result.state==='ready' && fs.readFileSync(path.join(state,'backend.pid'),'utf8')!==oldBackend;
      },'server recovers after failure backoff',45000);
      const denied=await river.frame.evaluate(async target=>{
        try{const socket=await fixture.platform.sessions.connect({action:'resume',target});socket.close();return 'accepted';}
        catch(error){return error.code;}
      },{session_id:room.session});
      assert(['session_ended','resume_expired'].includes(denied),'A lost room must report an ended/expired resume: '+denied);
      room=await play(river);await play(sky,{action:'join',target:{session_id:room.session}});
    }
    await moving(river,sky);
    assert.equal((await commit(river,checkpoint)).revision,revision,'Retry survives disruption without a second checkpoint');
    assert.deepEqual((await load(river)).files,checkpoint.files);
    assert.equal((await load(sky)).revision,null,'Other profile never acquires the owner save');
    assert.equal((await load(guest)).revision,guestWrite.revision,'Guest progress stays isolated');
    evidence('recovered',{cycle:cycles,fault,recovery_seconds:Math.round((Date.now()-cycleStarted)/1000),...resources()});
    const rest=Math.min(interval,duration-(Date.now()-started));
    for(let remaining=rest;remaining>0;) {
      const delay=Math.min(15000,remaining);await sleep(delay);remaining-=delay;
      await moving(river,sky); // Assert actual gameplay, not just open sockets.
    }
  }
  await river.frame.evaluate(()=>fixture.socket.send(JSON.stringify({type:'finish'})));
  await Promise.all([river,sky].map(game=>game.frame.waitForFunction(()=>!fixture.socket)));
  evidence('passed',{cycles,...resources()});complete=true;
} finally {
  await cleanup();
}
