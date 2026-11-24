// Run from platform_journey with INFUSER_DIAGNOSTICS_BROWSER_NODE. Only local fixture traffic.
import assert from 'node:assert/strict';
import http from 'node:http';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'../services/peerjs/node_modules/playwright/index.mjs');
const [base,host,ticket]=process.argv.slice(2);
const browser=await chromium.launch({channel:'chromium',executablePath:process.env.INFUSER_CHROMIUM,headless:true});
try {
  const ctx=await browser.newContext();
  await ctx.addCookies([{name:'__Host-infuser_game',value:ticket,url:'https://'+host,secure:true,httpOnly:true}]);
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
          route.fulfill({status:res.statusCode,headers:h,body:Buffer.concat(chunks)}).then(resolve,reject);
        });
      });proxy.on('error',reject);proxy.end(req.postDataBuffer()||undefined);
    });
  });
  const page=await ctx.newPage();
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
  await page.close();
}finally{await browser.close();}
