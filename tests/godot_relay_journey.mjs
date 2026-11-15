// Opt-in live journey. Pass a private JSON file containing two fresh hidden-game
// preview URLs: {"urls":["https://...#infuser=...", "https://...#infuser=..."]}.
// These are short-lived game grants, never Google or publisher credentials.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT || 'playwright');
const config=JSON.parse(await fs.readFile(process.argv[2],'utf8'));
assert.equal(config.urls.length,2);
const forced=process.argv[3]!=='direct';
const browser=await chromium.launch({headless:true,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader','--disable-background-timer-throttling','--disable-renderer-backgrounding']});
const clients=[], errors=[];
try {
  for(const raw of config.urls) {
    const context=await browser.newContext({viewport:{width:1000,height:740}});
    await context.addInitScript(()=>{
      // Test-only selected-pair evidence; Godot owns all channels and RPC traffic.
      window.testPeerConnections=[];
      const Original=window.RTCPeerConnection;
      window.RTCPeerConnection=class extends Original {
        constructor(config){super(config);window.testPeerConnections.push(this);}
      };
    });
    const page=await context.newPage();
    page.on('pageerror',e=>errors.push(e.message));
    page.on('console',m=>{if(m.type()==='error' && /SCRIPT ERROR|Parse Error|Failed to load script/.test(m.text()))errors.push(m.text());});
    const url=new URL(raw);url.searchParams.set('autoplay','1');url.searchParams.set('multiplayer','1');
    if(forced)url.searchParams.set('relay','1');
    await page.goto(url.href);
    await page.frameLocator('#game').locator('canvas').waitFor();
    const frame=await (await page.locator('#game').elementHandle()).contentFrame();
    await frame.waitForFunction(()=>window.lanternLake?.multiplayer,{}, {timeout:30000});
    clients.push({page,frame});
  }
  const evidence=[];
  for(const client of clients) {
    await client.frame.waitForFunction(()=>window.lanternLake?.received>=10,{}, {timeout:35000});
    const state=await client.frame.evaluate(()=>window.lanternLake);
    const links=await client.frame.evaluate(async()=>{
      const result=[];
      for(const pc of window.testPeerConnections) {
        if(pc.connectionState!=='connected')continue;
        const stats=await pc.getStats();
        for(const report of stats.values())if(report.type==='transport'&&report.selectedCandidatePairId){
          const pair=stats.get(report.selectedCandidatePairId);
          result.push({policy:pc.getConfiguration().iceTransportPolicy,local:stats.get(pair.localCandidateId).candidateType,remote:stats.get(pair.remoteCandidateId).candidateType,sent:pair.bytesSent,received:pair.bytesReceived});
        }
      }return result;
    });
    assert.equal(links.length,1);
    assert.equal(links[0].policy,forced?'relay':'all');
    if(forced){assert.equal(links[0].local,'relay');assert.equal(links[0].remote,'relay');}
    assert.ok(links[0].sent>0&&links[0].received>0);
    evidence.push({role:state.role,session:state.session,rpc_received:state.received,links});
  }
  assert.equal(evidence[0].session,evidence[1].session);
  assert.deepEqual(evidence.map(e=>e.role),['host','member']);
  await fs.mkdir('.local/artifacts',{recursive:true});
  await clients[1].page.screenshot({path:'.local/artifacts/godot-relay-multiplayer.jpg',type:'jpeg',quality:85});
  await clients[1].page.locator('#fullscreen').click();
  await clients[1].page.waitForFunction(()=>document.fullscreenElement!==null);
  // Host departure must end the existing room, never silently create a second one.
  await clients[0].frame.locator('canvas').click({position:{x:800,y:400}});
  await clients[0].page.keyboard.press('l');
  await clients[1].frame.waitForFunction(()=>window.lanternLake.multiplayer===false);
  const stopped=await clients[1].frame.evaluate(()=>window.lanternLake);
  assert.match(stopped.status,/ended|lost|left|session/i);
  assert.equal(await clients[1].page.evaluate(()=>document.fullscreenElement!==null),true,'connection status must not exit fullscreen');
  assert.deepEqual(errors,[]);
  console.log(JSON.stringify({mode:forced?'forced-relay':'automatic',evidence,host_departure:'session ended'},null,2));
} finally {
  for(const c of clients)await c.frame.evaluate(()=>fetch('/_infuser/api/v1/heartbeat',{method:'DELETE'})).catch(()=>{});
  await browser.close();
}
