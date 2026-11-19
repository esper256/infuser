// Opt-in live journey. Pass a private JSON file containing two fresh hidden-game
// preview URLs: {"urls":["https://...#infuser=...", "https://...#infuser=..."]}.
// These are short-lived game grants, never Google or publisher credentials.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT || 'playwright');
const config=JSON.parse(await fs.readFile(process.argv[2],'utf8'));
assert.equal(config.urls.length,2);
const forced=process.argv[3]!=='direct';
const recovery=process.argv[3]==='recovery';
const browser=process.env.INFUSER_BROWSER_CDP ? await chromium.connectOverCDP(process.env.INFUSER_BROWSER_CDP) : await chromium.launch({headless:true,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader','--disable-background-timer-throttling','--disable-renderer-backgrounding']});
const clients=[], errors=[];
const renewals=[];
try {
  for(const raw of config.urls) {
    const context=await browser.newContext({viewport:{width:1000,height:740}});
    const renewal={attempts:0,successes:0,initial:'',latest:''};renewals.push(renewal);
    if(recovery)await context.route('**/_infuser/api/v1/peers**',async route=>{
      const request=route.request(),path=new URL(request.url()).pathname;
      if(request.method()!=='POST'||!(path.endsWith('/peers')||path.endsWith('/ice')))return route.continue();
      if(path.endsWith('/ice')){
        renewal.attempts++;
        if(renewal.attempts===1)return route.fulfill({status:503,contentType:'application/json',body:JSON.stringify({error:'Test transient renewal failure'})});
      }
      const response=await route.fetch();const body=await response.json();
      if(response.ok()&&body.ice_servers){
        if(path.endsWith('/peers')){
          renewal.initial=body.ice_servers[0].username;
          // Exercise the real deadline logic without an hour-long CI test.
          // Credentials themselves are unmodified and really issued by TURN.
          body.ice_servers[0].expires_at=Math.floor(Date.now()/1000)+145;
        }else{renewal.successes++;renewal.latest=body.ice_servers[0].username;}
      }
      return route.fulfill({response,json:body});
    });
    await context.addInitScript(()=>{
      // Test-only selected-pair evidence; Godot owns all channels and RPC traffic.
      window.testPeerConnections=[];
      const Original=window.RTCPeerConnection;
      window.RTCPeerConnection=class extends Original {
        constructor(config){super(config);window.testPeerConnections.push(this);}
      };
    });
    const page=await context.newPage();
    const requests={polls:0,last_poll_status:0,leaves:0};
    page.on('response',response=>{const r=response.request();if(/\/peers\/[a-f0-9]+(?:\?|$)/.test(r.url())){if(r.method()==='GET'){requests.polls++;requests.last_poll_status=response.status();}if(r.method()==='DELETE')requests.leaves++;}});
    page.on('pageerror',e=>errors.push(e.message));
    page.on('console',m=>{if(m.type()==='error' && /SCRIPT ERROR|Parse Error|Failed to load script/.test(m.text()))errors.push(m.text());});
    const url=new URL(raw);url.searchParams.set('autoplay','1');url.searchParams.set('multiplayer','1');url.searchParams.set('matchmaking','1');
    if(forced)url.searchParams.set('relay','1');
    await page.goto(url.href);
    await page.frameLocator('#game').locator('canvas').waitFor();
    const frame=await (await page.locator('#game').elementHandle()).contentFrame();
    await frame.waitForFunction(()=>window.lanternLake?.multiplayer,{}, {timeout:30000});
    if(process.env.INFUSER_EXPECT_THREADS==='1')assert.equal(await frame.evaluate(()=>crossOriginIsolated&&typeof SharedArrayBuffer==='function'),true,'threaded export needs an isolated frame');
    clients.push({page,frame,context,requests});
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
  if(recovery){
    const sameSession=evidence[0].session;
    const hidden=await clients[0].context.newPage();await hidden.goto('about:blank');await hidden.bringToFront();
    const backgroundBefore=await clients[1].frame.evaluate(()=>window.lanternLake.received);
    await new Promise(resolve=>setTimeout(resolve,4000));
    assert((await clients[1].frame.evaluate(()=>window.lanternLake.received))>backgroundBefore,'background host continues sending');
    await hidden.close();await clients[0].page.bringToFront();
    // Brief control-plane outage must recover without inventing another room.
    await clients[1].context.setOffline(true);
    await new Promise(resolve=>setTimeout(resolve,4000));
    await clients[1].context.setOffline(false);
    for(const c of clients)await c.frame.waitForFunction(epoch=>window.lanternLake.multiplayer&&window.lanternLake.session===epoch,sameSession);
    const deadline=Date.now()+60000;
    while(renewals.some(r=>r.successes===0)&&Date.now()<deadline)await new Promise(resolve=>setTimeout(resolve,250));
    for(const r of renewals){assert(r.attempts>=2);assert(r.successes>=1);assert.notEqual(r.initial,r.latest);}
    for(const c of clients){
      const before=await c.frame.evaluate(()=>window.lanternLake.received);
      await c.frame.waitForFunction(n=>window.lanternLake.received>n+10,before,{timeout:25000});
      assert.equal(await c.frame.evaluate(()=>window.lanternLake.session),sameSession);
    }
    // Kill the real transport while keeping the coordinator lease alive.
    await clients[0].frame.evaluate(()=>window.testPeerConnections.filter(pc=>pc.connectionState==='connected').forEach(pc=>pc.close()));
    const before=await clients[1].frame.evaluate(()=>window.lanternLake.received);
    await clients[1].frame.waitForFunction(n=>window.lanternLake.received>n+10,before,{timeout:25000});
    assert.equal(await clients[1].frame.evaluate(()=>window.lanternLake.session),sameSession);
    // Suspend the renderer past its lease. Chrome can exempt active WebRTC
    // pages from lifecycle freezing, so pause execution rather than assuming
    // that Page.setWebLifecycleState actually stopped the game's callbacks.
    const cdp=await clients[0].context.newCDPSession(clients[0].page);
    await cdp.send('Debugger.enable');
    await cdp.send('Debugger.pause');
    await new Promise(resolve=>setTimeout(resolve,32000));
    await cdp.send('Debugger.resume');
    for(const c of clients){await c.page.bringToFront();await c.frame.waitForFunction(()=>!window.lanternLake.multiplayer,{}, {timeout:10000});}
    assert.deepEqual(errors,[]);
    console.log(JSON.stringify({browser:browser.version(),mode:'recovery',brief_outage:'same session',credential_renewal:'retried and rotated',closed_transport:'reconnected',suspended_renderer:'session ended',renewal_attempts:renewals.map(r=>r.attempts)}));
    process.exitCode=0;
  }else{
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
  }
} catch(error) {
  console.error('Godot browser errors:',JSON.stringify(errors));
  for(const c of clients)console.error('Godot peer state:',JSON.stringify({requests:c.requests,...await c.frame.evaluate(()=>({visibility:document.visibilityState,game:window.lanternLake,connections:window.testPeerConnections.map(pc=>({state:pc.connectionState,signaling:pc.signalingState}))})).catch(()=>({unavailable:true}))}));
  throw error;
} finally {
  for(const c of clients)await c.frame.evaluate(()=>fetch('/_infuser/api/v1/heartbeat',{method:'DELETE'})).catch(()=>{});
  for(const c of clients)await c.context.close();
  await browser.close();
}
