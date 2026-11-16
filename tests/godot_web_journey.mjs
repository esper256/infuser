// Opt-in real Godot/WASM journey, not part of the fast SDK unit suite.
// Run the godot_fixture example first. Set INFUSER_PLAYWRIGHT to an installed
// Playwright entry point if it is not on Node's module search path.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import https from 'node:https';
function local(path, method, headers, body) {
  return new Promise((resolve,reject)=>{
    const req=http.request({hostname:'127.0.0.1',port:17877,path,method,headers},res=>{
      const chunks=[];res.on('data',c=>chunks.push(c));res.on('end',()=>resolve({status:res.statusCode,headers:Object.fromEntries(Object.entries(res.headers).filter(([k])=>!['transfer-encoding','connection','content-length'].includes(k)).map(([k,v])=>[k,Array.isArray(v)?v.join('\n'):v])),body:Buffer.concat(chunks)}));
    });req.on('error',reject);req.end(body);
  });
}
const {chromium} = await import(process.env.INFUSER_PLAYWRIGHT || 'playwright');
// A real TLS proxy also covers AudioWorklet requests, which browser routing misses.
const proxy=https.createServer({key:await fs.readFile('.local/godot/key.pem'),cert:await fs.readFile('.local/godot/cert.pem')},async(req,res)=>{
  try {
    const chunks=[];for await(const chunk of req)chunks.push(chunk);
    const result=await local(req.url,req.method,req.headers,Buffer.concat(chunks));
    res.writeHead(result.status,result.headers);res.end(result.body);
  }catch {res.writeHead(502);res.end();}
});
await new Promise(resolve=>proxy.listen(17878,'127.0.0.1',resolve));
const browser = await chromium.launch({headless:true,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader','--no-proxy-server','--host-resolver-rules=MAP *.games.example.com 127.0.0.1:17878, MAP games.example.com 127.0.0.1:17878']});
const errors=[];
const existing=process.env.INFUSER_GODOT_DEMO==='dodge';
try {
  const context=await browser.newContext({viewport:{width:1000,height:740},ignoreHTTPSErrors:true});
  async function launch(guest=false) {
    const r=await local('/api/apps/lantern-lake/launch','POST',{host:'games.example.com',origin:'https://games.example.com',cookie:'member=admin','Content-Type':'application/json'},JSON.stringify({guest}));
    assert.equal(r.status,200,r.body.toString()); const launch=JSON.parse(r.body);
    const page=await context.newPage();
    page.on('pageerror',e=>errors.push(e.message));
    page.on('console',m=>{if(m.type()==='error' && /SCRIPT ERROR|Parse Error|Failed to load script/.test(m.text()))errors.push(m.text());});
    await page.goto(launch.url);
    const frame=await game(page);
    return {page,frame};
  }
  async function game(page) {
    await page.waitForSelector('iframe#game');
    const frame=await page.locator('iframe#game').contentFrame();
    // FrameLocator cannot evaluate; get the actual Frame once the iframe navigates.
    await frame.locator('canvas').waitFor();
    const actual=await (await page.locator('#game').elementHandle()).contentFrame();
    await actual.waitForFunction(existing=>existing?window.dodgeProbe?.ready:window.lanternLake?.loaded,existing,{timeout:30000});
    await actual.locator('canvas').click(existing?{}:{position:{x:800,y:400}});
    return actual;
  }
  async function state(client) { return client.frame.evaluate(()=>window.lanternLake); }
  async function collect(client, count=1) {
    const before=(await state(client)).lanterns;
    for(let i=0;i<count;i++)await client.page.keyboard.press('c');
    await client.frame.waitForFunction(n=>window.lanternLake.lanterns===n,before+count);
  }
  async function save(client) {
    const before=(await state(client)).revision;
    await client.page.keyboard.press('s');
    await client.frame.waitForFunction(old=>window.lanternLake.status==='Saved!'&&window.lanternLake.revision!==old,before);
  }
  if(existing){
    const owner=await launch();
    assert.equal(await owner.frame.evaluate(()=>window.dodgeProbe.best),0);
    await owner.page.keyboard.press('Enter');
    await owner.frame.waitForFunction(()=>window.dodgeProbe.score>=2,{}, {timeout:20000});
    await owner.frame.waitForFunction(()=>window.dodgeProbe.ready&&window.dodgeProbe.best>=2&&window.dodgeProbe.revision,{}, {timeout:60000});
    const best=await owner.frame.evaluate(()=>window.dodgeProbe.best);
    await owner.page.reload();owner.frame=await game(owner.page);
    assert.equal(await owner.frame.evaluate(()=>window.dodgeProbe.best),best);
    const guest=await launch(true);
    assert.equal(await guest.frame.evaluate(()=>window.dodgeProbe.guest),true);
    assert.equal(await guest.frame.evaluate(()=>window.dodgeProbe.best),0);
    const again=await launch();
    assert.equal(await again.frame.evaluate(()=>window.dodgeProbe.best),best);
    await fs.mkdir('.local/artifacts',{recursive:true});
    await again.page.screenshot({path:'.local/artifacts/godot-dodge-migration.png'});
    assert.deepEqual(errors,[]);
    console.log('Existing Dodge the Creeps: original gameplay, game-over checkpoint, reload, empty guest and preserved owner passed.');
  }else{
  const owner=await launch();
  assert.equal((await state(owner)).lanterns,0);
  assert.equal((await state(owner)).guest,false);
  await collect(owner,3); await save(owner);
  await owner.page.reload(); owner.frame=await game(owner.page);
  assert.equal((await state(owner)).lanterns,3);
  const guest=await launch(true);
  assert.equal((await state(guest)).guest,true);
  assert.equal((await state(guest)).lanterns,0);
  await collect(guest,2); await save(guest);
  await guest.page.reload(); guest.frame=await game(guest.page);
  assert.equal((await state(guest)).lanterns,2,'same guest launch survives refresh');
  const fresh=await launch(true);
  assert.equal((await state(fresh)).lanterns,0,'new guest starts empty');
  const ownerAgain=await launch();
  assert.equal((await state(ownerAgain)).lanterns,3,'guest never overwrites owner');
  // Real Godot checkpoint conflict: two copies load the same revision.
  await owner.frame.locator('canvas').click({position:{x:800,y:400}});
  await collect(owner); await save(owner);
  await collect(ownerAgain,2);
  await ownerAgain.page.keyboard.press('s');
  await ownerAgain.frame.waitForFunction(()=>window.lanternLake.status.startsWith('Another save arrived'));
  const conflict=await ownerAgain.frame.evaluate(async()=> (await fetch('/_infuser/api/v1/cloud/v1/groups/progress')).json());
  assert.equal(conflict.files['progress.json'].lanterns,4);
  assert.ok(conflict.conflicts.length>0,'conflicting candidate preserved');
  // The reusable Godot chooser focuses the cloud copy; choose it explicitly.
  await ownerAgain.frame.waitForFunction(()=>window.lanternLake.save_choice_ready);
  await ownerAgain.page.keyboard.press('Enter');
  await ownerAgain.frame.waitForFunction(()=>window.lanternLake.status==='Saved!'&&window.lanternLake.lanterns===4);
  const resolved=await ownerAgain.frame.evaluate(async()=> (await fetch('/_infuser/api/v1/cloud/v1/groups/progress')).json());
  assert.equal(resolved.conflicts.length,0);
  assert.equal(resolved.files['progress.json'].lanterns,4);
  assert.notEqual(resolved.revision,conflict.revision);
  // An explicit error must not display success or forget the retry's commit ID.
  const attempts=[]; let fail=true;
  await fresh.page.route('**/_infuser/api/v1/cloud/v1/groups/progress',async route=>{
    if(route.request().method()==='POST') {
      attempts.push(route.request().postDataJSON().commit_id);
      if(fail){fail=false;return route.fulfill({status:403,contentType:'application/json',body:JSON.stringify({error:'Test save interruption'})});}
    }
    return route.fallback();
  });
  await fresh.frame.locator('canvas').click({position:{x:800,y:400}});
  await collect(fresh); await fresh.page.keyboard.press('s');
  await fresh.frame.waitForFunction(()=>window.lanternLake.status.startsWith('Not saved:'));
  await save(fresh);
  assert.equal(attempts.length,2); assert.equal(attempts[0],attempts[1]);
  await fs.mkdir('.local/artifacts',{recursive:true});
  await owner.page.screenshot({path:'.local/artifacts/godot-lantern-lake.jpg',type:'jpeg',quality:85});
  assert.deepEqual(errors,[]);
  console.log('Godot/WASM: save + reload, guest refresh, fresh guest, owner isolation, conflict chooser, error + idempotent retry passed.');
  }
} finally { await browser.close(); proxy.closeAllConnections(); await new Promise(resolve=>proxy.close(resolve)); }
