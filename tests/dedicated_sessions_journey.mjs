// Real browser + Node ws + production Rust gateway. Run dedicated_fixture first.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {startProxy,chromium,browserOptions} from './support/dedicated_browser.mjs';
const config=JSON.parse(fs.readFileSync(process.argv[2]));
const proxy=await startProxy(config);
const origin=`https://games.example.com:${proxy.address().port}`;
const browser=await chromium.launch(browserOptions);
const errors=[];
async function member(name){
  const context=await browser.newContext({ignoreHTTPSErrors:true});
  await context.addCookies([{name:'member',value:name,url:origin}]);return context;
}
async function launch(context,options={}){
  // Browser fetch uses Chromium's resolver rules; Playwright's API client does not.
  const main=await context.newPage();await main.goto(origin+'/library');
  const result=await main.evaluate(async options=>{const r=await fetch('/api/apps/dedicated-lab/launch',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(options)});return {status:r.status,body:await r.json()};},options);
  assert.equal(result.status,200,JSON.stringify(result.body));
  const page=await context.newPage();page.on('pageerror',error=>errors.push(error.message));
  await page.goto(result.body.url);return opened(main,page);
}
async function opened(main,page){
  await page.locator('#game').waitFor();
  const frame=await (await page.locator('#game').elementHandle()).contentFrame();
  await frame.waitForFunction(()=>!!window.fixture);return {main,page,frame};
}
async function joinFromLibrary(context,avatar=false){
  const main=await context.newPage();await main.goto(origin+'/library');
  let menu=main;
  if(avatar){await main.getByLabel('River · online',{exact:true}).click();}
  else{
    await main.getByRole('button',{name:'About Dedicated Server Lab',exact:true}).click();
    menu=main.getByRole('dialog',{name:'Dedicated Server Lab',exact:true});
    await menu.getByLabel('Play options for Dedicated Server Lab',{exact:true}).click();
  }
  const next=context.waitForEvent('page');
  await menu.getByRole('button',{name:avatar?'Join Dedicated Server Lab':'Join River',exact:true}).click();
  const page=await next;await page.waitForURL('**/_infuser/play');return opened(main,page);
}
async function play(game,button='join'){
  await game.frame.locator('#'+button).click();
  try{await game.frame.waitForFunction(()=>fixture.socket?.readyState===1 && fixture.id,{},{timeout:15000});}
  catch(error){throw new Error('Admission: '+await game.frame.locator('#status').innerText(),{cause:error});}
  return game.frame.evaluate(()=>({id:fixture.id,session:fixture.socket.infuser.session.id}));
}
async function publish(path,method='GET',body){
  const raw=Buffer.isBuffer(body);
  const response=await fetch(config.api+'/api/publisher/apps/dedicated-lab'+path,{method,headers:{Authorization:'Bearer '+config.token,...(body===undefined?{}:{'Content-Type':raw?'application/octet-stream':'application/json'})},body:body===undefined?undefined:raw?body:JSON.stringify(body)});
  const result=await response.json();assert(response.ok,JSON.stringify(result));return result;
}
async function until(check,label,timeout=20000){
  const deadline=Date.now()+timeout;while(Date.now()<deadline){if(await check())return;await new Promise(r=>setTimeout(r,200));}throw Error('Timed out: '+label);
}
const connectRaw=(game,options)=>game.frame.evaluate(async options=>{try{window.extraSocket=await fixture.platform.sessions.connect(options);return {session:extraSocket.infuser.session.id};}catch(error){return {error:error.code};}},options);
try{
  const river=await member('river'),sky=await member('sky');
  const a=await launch(river);const first=await play(a);
  console.log('Dedicated: cold start, verified identity and first welcome frame passed.');
  const presence=await a.frame.evaluate(()=>fixture.platform.presence.get());
  assert(presence.sessions.some(s=>s.id===first.session&&s.players.some(p=>p.name==='River')));
  let b=await joinFromLibrary(sky);
  await b.frame.waitForFunction(()=>fixture.socket?.readyState===1 && fixture.id);
  assert.equal(await b.frame.evaluate(()=>fixture.socket.infuser.session.id),first.session);
  await a.frame.waitForFunction(()=>fixture.state.players.length===2);
  await b.frame.evaluate(()=>fixture.leave());
  b=await joinFromLibrary(sky,true);await b.frame.waitForFunction(()=>fixture.socket?.readyState===1 && fixture.id);
  assert.equal(await b.frame.evaluate(()=>fixture.socket.infuser.session.id),first.session);
  console.log('Dedicated: Library card and avatar Join Friend, plus unchanged gameplay messages passed.');
  const guest=await launch(river,{guest:true});
  await guest.frame.getByRole('button',{name:'Join River',exact:true}).click();
  await guest.frame.waitForFunction(()=>fixture.socket?.readyState===1 && fixture.id);
  const visitor=await guest.frame.evaluate(()=>({id:fixture.id,session:fixture.socket.infuser.session.id}));
  assert.notEqual(visitor.id,first.id);assert.equal(visitor.session,first.session);
  assert.match(await guest.frame.locator('#status').innerText(),/Guest/);
  const own=await launch(sky,{guest:true});const solo=await play(own,'own');assert.notEqual(solo.session,first.session);
  const separate=await launch(river,{guest:true});const second=await play(separate,'new');assert.notEqual(second.session,first.session);
  console.log('Dedicated: guest identity, single player and explicit separate run passed.');
  // Lose network access without cancelling the launch or its reconnect reservation.
  await river.setOffline(true);
  await a.frame.evaluate(()=>fixture.socket.close());
  await new Promise(resolve=>setTimeout(resolve,3000));
  await river.setOffline(false);
  await a.frame.locator('#resume').waitFor({state:'visible'});
  const resumed=await play(a,'resume');assert.deepEqual(resumed,first);
  console.log('Dedicated: network loss/reconnect resumes the exact run and identity.');
  // A different launch cannot claim someone else's held seat.
  const stranger=await launch(sky,{guest:true});
  const denied=await stranger.frame.evaluate(async target=>{try{await fixture.platform.sessions.connect({action:'resume',target});return 'accepted';}catch(e){return e.code;}},{session_id:first.session});
  assert.equal(denied,'resume_expired');
  // Seven occupied seats, then two simultaneous claims for the last one.
  for(let n=0;n<4;n++)assert.equal((await connectRaw(await launch(sky,{guest:true}),{action:'join',target:{session_id:first.session}})).session,first.session);
  const rival=await launch(sky,{guest:true});
  const race=await Promise.all([stranger,rival].map(g=>connectRaw(g,{action:'join',target:{session_id:first.session}})));
  assert.equal(race.filter(r=>r.session).length,1);assert.equal(race.find(r=>r.error)?.error,'session_full');
  console.log('Dedicated: simultaneous last-seat admission has one winner and a readable session_full result.');
  // Logging out of the website is deliberately independent of a running game.
  await river.clearCookies({name:'member'});
  await a.frame.evaluate(()=>fixture.socket.send(JSON.stringify({type:'input',direction:0})));
  await new Promise(r=>setTimeout(r,2300));assert.equal(await a.frame.evaluate(()=>fixture.socket.readyState),1);
  console.log('Dedicated: website sign-out leaves existing play connected.');
  await separate.frame.evaluate(()=>fixture.socket.send(JSON.stringify({type:'finish'})));
  await separate.frame.waitForFunction(()=>!fixture.socket);
  const ended=await stranger.frame.evaluate(async target=>{try{await fixture.platform.sessions.connect({action:'join',target});return 'accepted';}catch(e){return e.code;}},{session_id:second.session});
  assert.equal(ended,'session_ended');
  const original=await publish('/builds/one');
  const manifest=structuredClone(original.manifest);manifest.sessions.policy='shared';
  // Exercise a declared non-root endpoint and WebSocket subprotocol on the update.
  manifest.server_connection={path:'/play',protocols:['infuser-fixture.v1'],max_message_bytes:65536};
  const next='two-'+Date.now();
  await publish('/builds/'+next,'PUT',{manifest,server:original.server});
  for(const file of Object.keys(original.files))await publish('/builds/'+next+'/files/'+file,'PUT',fs.readFileSync(config.state_dir+'/content/dedicated-lab/one/'+file));
  assert.equal((await publish('/builds/'+next+'/publish','POST',{listed:true})).state,'queued');
  const blocked=await connectRaw(separate,{action:'create',activity:'multiplayer'});assert.equal(blocked.error,'server_draining');
  assert.equal(await a.frame.evaluate(()=>fixture.socket.readyState),1);
  // Existing runs finish normally; their idle title screens must not block activation.
  await a.frame.evaluate(()=>fixture.socket.send(JSON.stringify({type:'finish'})));
  await own.frame.evaluate(()=>fixture.socket.send(JSON.stringify({type:'finish'})));
  await until(async()=>(await publish('/builds/'+next)).publication_state==='published','queued pair activated',45000);
  const stale=await a.frame.evaluate(async()=>{try{await fixture.platform.runtime.acquire();return 200;}catch(e){return e.status;}});assert.equal(stale,409);
  const fresh=await launch(sky,{guest:true});const shared=await play(fresh);
  assert.equal(await fresh.frame.evaluate(()=>fixture.socket.protocol),'infuser-fixture.v1');
  const sharedFriend=await launch(sky,{guest:true});assert.equal((await play(sharedFriend)).session,shared.session);
  const rejected=await connectRaw(await launch(sky,{guest:true}),{action:'create',activity:'multiplayer'});assert.equal(rejected.error,'session_exists');
  console.log('Dedicated: queued update drains, old clients are fenced, new path/subprotocol and shared policy work.');
  if(process.env.INFUSER_DEDICATED_LONG_TEST){
    const cdp=await fresh.page.context().newCDPSession(fresh.page);
    await cdp.send('Page.setWebLifecycleState',{state:'frozen'});
    await new Promise(r=>setTimeout(r,35000));
    await cdp.send('Page.setWebLifecycleState',{state:'active'});
    assert.equal(await fresh.frame.evaluate(()=>fixture.socket.readyState),1);
    assert.equal(await fresh.frame.evaluate(()=>fixture.socket.infuser.session.id),shared.session);
    console.log('Dedicated: frozen renderer survives beyond the startup lease through transport liveness.');
    await new Promise(r=>setTimeout(r,95000));
    assert.equal(await fresh.frame.evaluate(()=>fixture.socket.readyState),1);
    console.log('Dedicated: server credential renewal preserves the running session beyond initial token expiry.');
  }
  await fresh.frame.evaluate(()=>fixture.socket.send(JSON.stringify({type:'finish'})));
  assert.deepEqual(errors,[]);
  console.log('Dedicated: ended targets never create replacement rooms. Browser journey passed.');
}finally{await browser.close();proxy.closeAllConnections();await new Promise(resolve=>proxy.close(resolve));}
