// Optional Chromium journey against the real compatibility fixture. This tests
// the Unity JS bridge and hosting, not a compiled Unity engine or C# assembly.
import assert from 'node:assert/strict';
import http from 'node:http';
import https from 'node:https';
import fs from 'node:fs/promises';
const [base, ownerUrl, guestUrl, certFile, keyFile] = process.argv.slice(2);
const {chromium} = await import(process.env.INFUSER_PLAYWRIGHT || 'playwright');
// Real TLS traffic matters: Playwright route.fulfill bypasses the browser's
// native gzip/Brotli decoder and cannot verify Unity's streaming WASM path.
const proxy=https.createServer({cert:await fs.readFile(certFile),key:await fs.readFile(keyFile)},(req,res)=>{
  const upstream=http.request(base+req.url,{method:req.method,headers:req.headers},reply=>{res.writeHead(reply.statusCode,reply.headers);reply.pipe(res);});
  upstream.on('error',()=>{res.writeHead(502);res.end();});req.pipe(upstream);
});
await new Promise(resolve=>proxy.listen(0,'127.0.0.1',resolve));
const port=proxy.address().port;
const browser = await chromium.launch({headless:true,executablePath:process.env.INFUSER_CHROMIUM,
  args:['--no-proxy-server',`--host-resolver-rules=MAP *.games.example.com 127.0.0.1:${port}, MAP games.example.com 127.0.0.1:${port}`]});
const context = await browser.newContext({ignoreHTTPSErrors:true});
try {
  async function launch(url) {
    const page=await context.newPage();await page.goto(url);
    await page.waitForSelector('iframe#game');
    const frame=await (await page.locator('#game').elementHandle()).contentFrame();
    await frame.waitForFunction(()=>window.Infuser);
    await frame.evaluate(async()=>{
      const {createBridge}=await import('/infuser-unity.mjs');
      const pending=new Map();let n=0;
      const bridge=createBridge(m=>{if(m.id){pending.get(m.id)(m);pending.delete(m.id);}});
      window.unityCall=(op,args={})=>new Promise(resolve=>{const id=String(++n);pending.set(id,resolve);bridge.call(id,op,JSON.stringify(args));});
    });
    return {page,frame};
  }
  const call=async(c,op,args={})=>{
    const reply=await c.frame.evaluate(([op,args])=>window.unityCall(op,args),[op,args]);
    assert.equal(reply.ok,true,JSON.stringify(reply));return JSON.parse(reply.json);
  };
  const owner=await launch(ownerUrl),guest=await launch(guestUrl);
  assert.equal(await owner.frame.evaluate(()=>crossOriginIsolated),true);
  assert.equal((await call(owner,'ready')).hosted,true);
  assert.equal((await call(guest,'ready')).guest,true);
  const save={group:'unity',file:'profile.json'};
  assert.equal((await call(owner,'save.load',save)).exists,false);
  const first=await call(owner,'save.commit',{...save,expectedRevision:null,commitId:'unity-browser-owner-0001',json:'{"schema":1,"stars":7}',summary:'7 stars'});
  assert.equal(first.status,'saved');
  assert.equal((await call(guest,'save.load',save)).exists,false);
  await call(guest,'save.commit',{...save,expectedRevision:null,commitId:'unity-browser-guest-0001',json:'{"schema":1,"stars":2}',summary:'2 stars'});
  assert.equal(JSON.parse((await call(owner,'save.load',save)).json).stars,7);
  const conflict=await call(owner,'save.commit',{...save,expectedRevision:null,commitId:'unity-browser-owner-0002',json:'{"schema":1,"stars":9}',summary:'9 stars'});
  assert.equal(conflict.status,'conflict');
  assert.equal(JSON.parse((await call(owner,'save.read',{...save,revision:conflict.current_revision})).json).stars,7);
  assert.equal(JSON.parse((await call(owner,'save.read',{...save,revision:conflict.candidate_revision})).json).stars,9);
  const resolved=await call(owner,'save.commit',{...save,expectedRevision:conflict.current_revision,candidateRevision:conflict.candidate_revision,commitId:'unity-browser-resolve-0001',json:'{"schema":1,"stars":9}',summary:'9 stars'});
  assert.equal(resolved.status,'saved');
  assert.equal(JSON.parse((await call(guest,'save.load',save)).json).stars,2);
  await owner.frame.evaluate(async()=>{
    for(const suffix of ['gz','br']) {
      const response=await fetch('/Build/probe.wasm.'+suffix);
      await WebAssembly.instantiateStreaming(response);
    }
  });
  // A new adapter instance rehydrates the same player's cloud save.
  await owner.page.close();const reopened=await launch(new URL('/_infuser/play',ownerUrl).href);
  assert.equal(JSON.parse((await call(reopened,'save.load',save)).json).stars,9);
  // Authentication loss remains an error; no empty/standalone save fallback.
  await context.clearCookies();
  const denied=await reopened.frame.evaluate(()=>window.unityCall('save.load',{group:'unity',file:'profile.json'}));
  assert.equal(denied.ok,false);assert.equal(denied.status,401);
  console.log('Unity bridge: Chromium gzip/Brotli WASM, real cloud conflict/resolve, reload, guest isolation and expired authentication passed.');
} finally {await browser.close();proxy.closeAllConnections();await new Promise(resolve=>proxy.close(resolve));}
