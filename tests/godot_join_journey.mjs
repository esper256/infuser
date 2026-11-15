// Opt-in hidden-game acceptance. INFUSER_GODOT_MINT is a private operator
// script returning {url}, accepting optional SESSION PLAYER arguments.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await chromium.connectOverCDP(process.env.INFUSER_BROWSER_CDP);
const clients=[];
const mint=(...args)=>JSON.parse(execFileSync(process.execPath,[process.env.INFUSER_GODOT_MINT,...args],{encoding:'utf8'})).url;
async function open(raw,host=false){
  const context=await browser.newContext();const page=await context.newPage();
  const url=new URL(raw);url.searchParams.set('autoplay','1');url.searchParams.set('relay','1');if(host)url.searchParams.set('multiplayer','1');
  await page.goto(url.href);await page.frameLocator('#game').locator('canvas').waitFor();
  const frame=await (await page.locator('#game').elementHandle()).contentFrame();
  await frame.waitForFunction(()=>window.lanternLake?.loaded,{}, {timeout:30000});
  const client={context,page,frame};clients.push(client);return client;
}
async function call(c,path,method='GET'){
  return c.frame.evaluate(async({path,method})=>{const r=await fetch('/_infuser/api/v1'+path,{method});if(!r.ok)throw Error('Game API '+r.status);return r.json();},{path,method});
}
try{
  const host=await open(mint(),true);await host.frame.waitForFunction(()=>window.lanternLake.multiplayer);
  const {session}=await call(host,'/presence/v1');assert.equal(session.joinable,true);
  const joined=await open(mint(session.id,session.host_player_id));
  for(const c of [host,joined])await c.frame.waitForFunction(()=>window.lanternLake.received>=10,{}, {timeout:35000});
  assert.equal(await joined.frame.evaluate(()=>window.lanternLake.session),session.id);
  assert.equal((await call(joined,'/launch-intent/v1/consume','POST')).intent,null);
  const stale=mint(session.id,session.host_player_id);
  await joined.page.reload();await joined.page.frameLocator('#game').locator('canvas').waitFor();
  joined.frame=await (await joined.page.locator('#game').elementHandle()).contentFrame();
  await joined.frame.waitForFunction(()=>window.lanternLake?.loaded,{}, {timeout:30000});
  assert.equal(await joined.frame.evaluate(()=>window.lanternLake.multiplayer),false,'refresh cannot repeat a consumed intent');
  await call(host,'/heartbeat','DELETE');await host.context.close();clients.splice(clients.indexOf(host),1);
  const late=await open(stale);
  await late.frame.waitForFunction(()=>!window.lanternLake.multiplayer&&window.lanternLake.status!=='Finding the shared lake…');
  assert.equal((await call(late,'/presence/v1')).session,null,'stale target creates no replacement room');
  console.log('Godot Join Friend: targeted relay join, one-time consumption, refresh stays at title screen and stale target creates no room passed.');
}finally{
  for(const c of clients){try{await call(c,'/heartbeat','DELETE');}catch{}await c.context.close();}
  await browser.close();
}
