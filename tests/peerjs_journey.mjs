// Opt-in real Chromium + official PeerJS/PeerServer acceptance. No public matchmaker.
// npm ci --prefix services/peerjs; cargo build --example peerjs_fixture
// node tests/peerjs_journey.mjs [private-live-urls.json] [recovery]
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {chromium} from '../services/peerjs/node_modules/playwright/index.mjs';
const processes=[], errors=[], clients=[];
const live=process.argv[2], recovery=process.argv[3]==='recovery';
async function start(command,args,ready) {
  const child=spawn(command,args,{stdio:['ignore','pipe','pipe']});processes.push(child);
  let output='';
  await new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>reject(new Error('Fixture startup timed out: '+output)),15000);
    child.once('exit',()=>{clearTimeout(timer);reject(new Error('Fixture exited: '+output));});
    for(const stream of [child.stdout,child.stderr])stream.on('data',b=>{output+=b;if(output.includes(ready)){clearTimeout(timer);resolve();}});
  });
}
let browser;
try {
  if(!live){await start(process.execPath,['services/peerjs/server.mjs'],'ready');await start('target/debug/examples/peerjs_fixture',[],'fixture ready');}
  browser=process.env.INFUSER_BROWSER_CDP ? await chromium.connectOverCDP(process.env.INFUSER_BROWSER_CDP) : await chromium.launch({headless:true,args:['--disable-background-timer-throttling','--disable-renderer-backgrounding']});
  const urls=live?JSON.parse(await fs.readFile(live,'utf8')).urls:['http://127.0.0.1:17878/test?autoplay','http://127.0.0.1:17878/test?autoplay'];
  for(let i=0;i<2;i++){
    const context=await browser.newContext();
    if(!live)await context.addCookies([{name:'fixture_player',value:i?'b':'a',url:'http://127.0.0.1:17878'}]);
    let renewals=0,attempts=0;
    if(recovery)await context.route('**/_infuser/api/v1/peers**',async route=>{
      const request=route.request(),path=new URL(request.url()).pathname;
      if(request.method()!=='POST'||!(path.endsWith('/peers')||path.endsWith('/ice')))return route.continue();
      if(path.endsWith('/ice')&&++attempts===1)return route.fulfill({status:503,contentType:'application/json',body:'{"error":"test transient renewal failure"}'});
      const response=await route.fetch(),body=await response.json();
      if(response.ok()&&body.ice_servers){if(path.endsWith('/peers'))body.ice_servers[0].expires_at=Math.floor(Date.now()/1000)+125;else renewals++;}
      await route.fulfill({response,json:body});
    });
    const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
    const url=new URL(urls[i]);url.searchParams.set('autoplay','1');if(live)url.searchParams.set('relay','1');
    await page.goto(url.href);
    const frame=live?await (await page.locator('#game').elementHandle()).contentFrame():page;
    await frame.waitForFunction(()=>window.peerjsTest?.room?.active,{}, {timeout:20000});
    clients.push({context,page,frame,renewals:()=>renewals,attempts:()=>attempts});
  }
  for(const c of clients)await c.frame.waitForFunction(()=>window.peerjsTest.received>10&&window.peerjsTest.binary>0,{}, {timeout:30000});
  const evidence=[];
  for(const c of clients){
    const e=await c.frame.evaluate(async()=>({role:peerjsTest.role,session:peerjsTest.session,received:peerjsTest.received,binary:peerjsTest.binary,links:await peerjsTest.room.stats()}));
    assert.equal(e.links.length,1);assert(e.links[0].bytesSent>0&&e.links[0].bytesReceived>0);
    if(live){assert.equal(e.links[0].local,'relay');assert.equal(e.links[0].remote,'relay');}
    evidence.push(e);
  }
  assert.deepEqual(evidence.map(e=>e.role),['host','member']);assert.equal(evidence[0].session,evidence[1].session);
  // Shut the real PeerJS data channel; keep the coordinator lease. A new real
  // DataConnection must resume messages in the same shared session.
  const before=await clients[1].frame.evaluate(()=>({received:peerjsTest.received,opens:peerjsTest.opens}));
  await clients[1].frame.evaluate(()=>peerjsTest.room.connections[0].close());
  await clients[1].frame.waitForFunction(v=>peerjsTest.opens>v.opens&&peerjsTest.received>v.received+5,before,{timeout:25000});
  assert.equal(await clients[1].frame.evaluate(()=>peerjsTest.session),evidence[0].session);
  if(recovery){
    const deadline=Date.now()+30000;
    while(clients.some(c=>c.renewals()===0)&&Date.now()<deadline)await new Promise(r=>setTimeout(r,100));
    for(const c of clients){assert(c.renewals()>0);assert(c.attempts()>=2);}
    for(const c of clients){const count=await c.frame.evaluate(()=>peerjsTest.received);await c.frame.waitForFunction(n=>peerjsTest.received>n+10,count);}
    // Cut signaling/network briefly, then require a fresh data connection.
    await clients[1].context.setOffline(true);
    await new Promise(r=>setTimeout(r,3500));
    await clients[1].context.setOffline(false);
    const count=await clients[1].frame.evaluate(()=>{for(const c of peerjsTest.room.connections)c.close();return peerjsTest.received;});
    await clients[1].frame.waitForFunction(n=>peerjsTest.received>n+10,count,{timeout:25000});
    assert.equal(await clients[1].frame.evaluate(()=>peerjsTest.session),evidence[0].session);
  }
  // Host departure ends the member's session; it must never silently host.
  await clients[0].frame.evaluate(()=>peerjsTest.room.close());
  await clients[1].frame.waitForFunction(()=>!!peerjsTest.ended,{}, {timeout:8000});
  assert.equal(await clients[1].frame.evaluate(()=>peerjsTest.room.active),false);
  if(!live){
    for(const c of clients)await c.context.close();clients.length=0;
    for(const query of ['id=standalone-host','id=standalone-member&friend=standalone-host']){
      const context=await browser.newContext(),page=await context.newPage();let platformRequests=0;
      page.on('request',r=>{if(new URL(r.url()).pathname.startsWith('/_infuser/'))platformRequests++;});
      await page.goto('http://127.0.0.1:17878/standalone?autoplay&'+query);
      clients.push({context,page,frame:page,requests:()=>platformRequests});
    }
    for(const c of clients){await c.page.waitForFunction(()=>peerjsTest.received>10&&peerjsTest.binary>0);assert.equal(c.requests(),0);assert.equal(await c.page.evaluate(()=>peerjsTest.platform),'standalone');}
  }
  assert.deepEqual(errors,[]);
  console.log(JSON.stringify({passed:true,mode:live?'forced-relay':'local-and-standalone',recovery,evidence},null,2));
} finally {for(const c of clients)await c.context.close();await browser?.close();for(const p of processes)p.kill();}
