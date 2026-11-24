// Run from platform_journey with INFUSER_DIAGNOSTICS_BROWSER_NODE. Only local fixture traffic.
import assert from 'node:assert/strict';
import http from 'node:http';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'../services/peerjs/node_modules/playwright/index.mjs');
const [base,host,ticket]=process.argv.slice(2);
const browser=await chromium.launch({channel:'chromium',executablePath:process.env.INFUSER_CHROMIUM,headless:true});
try {
  const ctx=await browser.newContext();
  await ctx.addCookies([{name:'__Host-infuser_game',value:ticket,url:'https://'+host,secure:true,httpOnly:true}]);
  let emergencyGeneration=0;
  await ctx.route('**/*',async route=>{
    const req=route.request(),url=new URL(req.url());
    if(url.hostname!==host)return route.abort();
    if(url.pathname==='/trace-fixture.js')return route.fulfill({contentType:'text/javascript',body:
      "console.error('Browser fixture crash');Promise.reject(new Error('Browser fixture crash'));throw new TypeError('Browser fixture crash');"});
    const headers={...await req.allHeaders(),host};
    const target=new URL(url.pathname+url.search,base);
    await new Promise((resolve,reject)=>{
      const proxy=http.request(target,{method:req.method(),headers},res=>{
        const chunks=[];res.on('data',c=>chunks.push(c));res.on('end',()=>{
          const h={...res.headers};delete h['transfer-encoding'];delete h.connection;
          let body=Buffer.concat(chunks);
          if(url.pathname==='/_infuser/play' && emergencyGeneration){
            body=Buffer.from(body.toString().replace('data-force-reload="0"',`data-force-reload="${emergencyGeneration}"`));
            delete h['content-length'];
          }
          route.fulfill({status:res.statusCode,headers:h,body}).then(resolve,reject);
        });
      });proxy.on('error',reject);proxy.end(req.postDataBuffer()||undefined);
    });
  });
  const page=await ctx.newPage();
  await page.addInitScript(()=>{
    if(window===parent)return;
    const interval=window.setInterval,clear=window.clearInterval;
    const probe=window.heartbeatProbe={clears:0,errors:0};
    window.setInterval=function(fn,ms,...args){
      if(ms!==25000)return interval.call(this,fn,ms,...args);
      probe.tick=fn;return -25000;
    };
    window.clearInterval=function(id){if(id===-25000)probe.clears++;else clear.call(this,id);};
    addEventListener('infuser-session-error',e=>{if(e.detail.status===401)probe.errors++;});
  });
  page.on('response',r=>{if(r.status()>=400)console.error('fixture HTTP',new URL(r.url()).pathname,r.status());});
  await page.goto('https://'+host+'/_infuser/play');
  await page.waitForFunction(()=>document.querySelector('#game')?.contentDocument?.title==='Moon v1');
  const kinds=['js.error','js.rejection','console.error'];
  const recorded=kinds.map(kind=>page.waitForRequest(req=>req.url().endsWith('/_infuser/telemetry')&&req.method()==='POST'&&
    req.postDataJSON().events.some(e=>e.kind===kind&&e.src==='game'&&e.data.file==='trace-fixture.js')));
  await page.waitForFunction(()=>document.querySelector('#game')?.contentWindow?.console.error.toString().includes('original.apply'));
  await page.frameLocator('#game').locator('body').evaluate(()=>{
    const script=document.createElement('script');script.src='/trace-fixture.js?private-fixture#redact';
    document.body.append(script);
  });
  for(const [i,req] of (await Promise.all(recorded)).entries()){
    const event=req.postDataJSON().events.find(e=>e.kind===kinds[i]&&e.data.file==='trace-fixture.js');
    assert.match(event.data.frames[0],/^trace-fixture\.js:1:\d+$/);
    assert.doesNotMatch(JSON.stringify(event),/private-fixture|Browser fixture crash|https?:|#redact/);
    assert.equal((await req.response()).status(),200);
  }
  // Headless Chromium keeps pages visible. Deliver the lifecycle event explicitly.
  const frozen=page.waitForRequest(req=>req.url().endsWith('/_infuser/telemetry')&&req.method()==='POST'&&req.postDataJSON()?.events?.some(e=>e.kind==='page.freeze'));
  await page.evaluate(()=>document.dispatchEvent(new Event('freeze')));
  assert.equal((await (await frozen).response()).status(),200);
  const frame=page.frameLocator('#game').locator('body');
  const current=await page.locator('meta[name="infuser-client-version"]').getAttribute('content');
  let client_update={version:'next',force_reload:0},update_available=false;
  await page.route('**/_infuser/heartbeat',r=>r.fulfill({json:{client_update,update_available}}));
  await frame.evaluate(()=>window.heartbeatProbe.tick());
  await page.getByText('Infuser has been updated. Reopen this game at your next break.',{exact:true}).waitFor();
  update_available=true;
  await frame.evaluate(()=>window.heartbeatProbe.tick());
  const title=await page.locator('#title').textContent();
  await page.getByText(`Infuser and ${title} have been updated. Reopen the game at your next break.`,{exact:true}).waitFor();
  client_update.version=current;
  await frame.evaluate(()=>window.heartbeatProbe.tick());
  const expected=`A new version of ${title} is ready. Reopen it at your next break.`;
  await page.getByText(expected,{exact:true}).waitFor();

  // Same-version emergency, delivered by the SDK. The replacement HTML acknowledges it.
  emergencyGeneration=1;client_update.force_reload=1;
  let navigations=0;
  page.on('framenavigated',f=>{if(f===page.mainFrame())navigations++;});
  const replaced=page.waitForEvent('framenavigated',f=>f===page.mainFrame());
  await frame.evaluate(()=>{void window.heartbeatProbe.tick();});
  await replaced;
  await page.getByText(expected,{exact:true}).waitFor();
  assert.equal(await page.locator('meta[name="infuser-client-version"]').getAttribute('data-force-reload'),'1');
  await frame.evaluate(()=>window.heartbeatProbe.tick());
  assert.equal(navigations,1);
  let heartbeatPosts=0;
  await page.route('**/_infuser/heartbeat',route=>{
    assert.equal(route.request().method(),'POST');heartbeatPosts++;
    return route.fulfill({status:401,contentType:'text/plain',body:'Expired'});
  });
  await frame.evaluate(async()=>{await window.heartbeatProbe.tick();await window.heartbeatProbe.tick();});
  const notice=page.locator('#status');
  await notice.getByRole('link',{name:'Return to your Library.'}).waitFor();
  assert.equal(await notice.textContent(),'This game session ended. Return to your Library.');
  assert.equal(await notice.locator('a').getAttribute('href'),await page.locator('#library').getAttribute('href'));
  assert.ok(await notice.evaluate(el=>{
    const link=el.firstElementChild;
    window.dispatchEvent(new MessageEvent('message',{origin:location.origin,source:document.querySelector('#game').contentWindow,
      data:{type:'infuser-session-error',status:401}}));
    return el.firstElementChild===link;
  }));
  assert.deepEqual(await frame.evaluate(()=>[window.heartbeatProbe.clears,window.heartbeatProbe.errors]),[1,1]);
  assert.equal(heartbeatPosts,1);
  // Exercise the same player with ordinary and visiting launch metadata.
  const session=await page.evaluate(async()=>(await fetch('/_infuser/context')).json());
  for(const [visit,status,reason] of [[false,410,'timeout'],[false,409,'server_ended'],[true,403,'visit_end']]){
    const player=await ctx.newPage();
    if(visit)await player.route('**/_infuser/context',route=>route.fulfill({
      json:{...session,visit:{expires_at:Math.floor(Date.now()/1000)+600,authorization_seconds:30}}
    }));
    await player.goto('https://'+host+'/_infuser/play');
    await player.waitForFunction(()=>document.querySelector('#game')?.contentDocument?.title==='Moon v1');
    if(visit){
      await player.route('**/_infuser/api/v1/heartbeat',r=>r.fulfill({json:{update_available:true,visit:{expires_at:Math.floor(Date.now()/1000)+90}}}));
      await player.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));
      await player.getByText('Visit ends in 2 min. Save your progress.',{exact:true}).waitFor();
      await player.evaluate(()=>window.dispatchEvent(new MessageEvent('message',{origin:location.origin,
        source:document.querySelector('#game').contentWindow,data:{type:'infuser-update-available'}})));
      assert.equal(await player.locator('#status').textContent(),'Visit ends in 2 min. Save your progress.');
    }
    await player.frameLocator('#game').locator('body').evaluate((_,status)=>{
      parent.postMessage({type:'infuser-session-error',status},location.origin);
    },status);
    const notice=player.locator('#status');
    await notice.getByRole('link',{name:'Return to your Library.'}).waitFor();
    const text=visit?'This visit has ended.':'This game session ended.';
    assert.equal(await notice.textContent(),text+' Return to your Library.');
    assert.equal(await player.locator('#game').isHidden(),visit);
    const ended=player.waitForRequest(req=>req.url().endsWith('/_infuser/telemetry')&&req.method()==='POST'&&
      req.postDataJSON().events.some(e=>e.kind==='link'&&e.data.name==='launch'&&e.data.state==='ended'));
    await player.evaluate(()=>document.dispatchEvent(new Event('freeze')));
    const row=(await ended).postDataJSON().events.find(e=>e.kind==='link'&&e.data.state==='ended'&&e.data.name==='launch');
    assert.equal(row.data.status,status);
    assert.equal(row.data.reason,reason);
    await player.close();
  }
  await page.close();
}finally{await browser.close();}
