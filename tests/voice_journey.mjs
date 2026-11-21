// Opt-in real LiveKit/browser acceptance. Uses only synthetic accounts and microphones.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {setTimeout as sleep} from 'node:timers/promises';
import {chromium,browserOptions,startProxy} from './support/dedicated_browser.mjs';
assert(process.env.INFUSER_TEST_LIVEKIT,'Set INFUSER_TEST_LIVEKIT to the verified livekit-server binary.');
const root=fs.mkdtempSync(path.join(os.tmpdir(),'infuser-voice-')),ready=path.join(root,'ready.json');
const log=fs.openSync(path.join(root,'fixture.log'),'w',0o600);
const fixture=spawn(process.env.INFUSER_VOICE_FIXTURE||'target/debug/examples/voice_fixture',[process.env.INFUSER_TEST_LIVEKIT,ready],{stdio:['ignore',log,log],detached:true});
let browser,proxy,passed=false;
try{
  for(let i=0;!fs.existsSync(ready)&&i<100;i++){assert.equal(fixture.exitCode,null,'Fixture failed: '+root);await sleep(100);}
  const config=JSON.parse(fs.readFileSync(ready));proxy=await startProxy(config);
  const origin=`https://games.example.com:${proxy.address().port}`;
  browser=await chromium.launch({...browserOptions,args:[...browserOptions.args,'--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream','--autoplay-policy=no-user-gesture-required']});
  async function player(who,visit){
    const context=await browser.newContext({ignoreHTTPSErrors:true,permissions:['microphone']});
    await context.addCookies([{name:'member',value:who,url:origin}]);
    await context.addInitScript(visit=>{
      if(visit)sessionStorage.setItem('infuser.visit',visit);
      window.connections=[];const Original=RTCPeerConnection;
      window.RTCPeerConnection=class extends Original{constructor(...args){super(...args);window.connections.push(this);}};
      window.voiceNotes=[];const start=OscillatorNode.prototype.start;
      OscillatorNode.prototype.start=function(...args){window.voiceNotes.push(this.frequency.value);return start.apply(this,args);};
    },visit);
    const page=await context.newPage();page.on('pageerror',e=>console.error(who,e.message));
    page.on('console',m=>{if(m.type()==='error')console.error(who,m.text().slice(0,500));});
    await page.goto(origin+'/library');
    await page.addLocatorHandler(page.getByRole('dialog',{name:'Profile picture',exact:true}),async()=>{await page.getByRole('button',{name:'Close profile picture picker'}).click();},{times:1});
    await page.locator('#voice-toggle').waitFor({state:'visible'});
    await page.waitForFunction(()=>!document.querySelector('#voice-toggle').disabled);
    return {page,context};
  }
  const river=await player('river'),sky=await player('sky');
  await river.page.locator('#voice-toggle').click();await sky.page.locator('#voice-toggle').click();
  for(const {page} of [river,sky]){
    await page.waitForFunction(()=>document.querySelector('#voice-toggle').dataset.state==='on',{},{timeout:20000});
    await page.waitForFunction(async()=>{for(const pc of window.connections){for(const s of (await pc.getStats()).values())if(s.type==='inbound-rtp'&&s.kind==='audio'&&s.bytesReceived>100&&s.totalAudioEnergy>0)return true;}return false;},{},{timeout:15000});
  }
  console.log('Two real browser microphones delivered decoded audio in both directions.');
  for(const {page} of [river,sky]){
    const notes=await page.evaluate(()=>window.voiceNotes);
    assert.equal(notes.length,3,'one entry melody, without announcing the existing roster');
    assert(notes[0]<notes[1]&&notes[1]<notes[2]);
  }
  await river.context.setOffline(true);await sleep(2500);await river.context.setOffline(false);
  await river.page.waitForFunction(()=>document.querySelector('#voice-toggle').dataset.state==='on',{},{timeout:20000});
  await river.page.waitForFunction(()=>document.querySelectorAll('.voice-badge').length===2);
  // Exercise Library split pill, mixer controls, mute badge, deafen, and focus handling.
  await river.page.locator('#voice-mixer-toggle').click();
  await river.page.locator('#voice-mixer').waitFor({state:'visible'});
  await river.page.screenshot({path:path.join(root,'voice-mixer.png')});
  await river.page.locator('#voice-mixer').getByRole('button',{name:'Mute',exact:true}).click();
  await river.page.waitForFunction(()=>document.querySelector('.voice-split')?.dataset.muted==='true'&&!document.querySelector('.voice-split-badge')?.hidden);
  await river.page.locator('#voice-mixer').getByRole('button',{name:'Deafen',exact:true}).click();
  await river.page.waitForFunction(()=>document.querySelector('.voice-split')?.dataset.deafened==='true'&&document.querySelector('.voice-split-badge')?.hidden);
  await river.page.locator('#voice-mixer').getByRole('button',{name:'Unmute',exact:true}).click();
  await river.page.waitForFunction(()=>document.querySelector('.voice-split')?.dataset.muted==='false'&&document.querySelector('.voice-split')?.dataset.deafened==='false');
  await river.page.keyboard.press('Escape');
  await river.page.locator('#voice-mixer').waitFor({state:'hidden'});
  assert.equal(await river.page.evaluate(()=>document.activeElement?.id),'voice-mixer-toggle','closing mixer returns focus to caret');
  await river.page.screenshot({path:path.join(root,'voice-header.png')});

  // Real 360x800 narrow-screen check for Library header + popover.
  await river.page.setViewportSize({width:360,height:800});
  await river.page.locator('#voice-mixer-toggle').click();
  await river.page.locator('#voice-mixer').waitFor({state:'visible'});
  const lib360=await river.page.evaluate(()=>{
    const header=document.querySelector('header');
    const pill=document.querySelector('.voice-split').getBoundingClientRect();
    const chat=document.querySelector('#chat-toggle').getBoundingClientRect();
    const pop=document.querySelector('#voice-mixer').getBoundingClientRect();
    return {
      headerOverflow:header.scrollWidth>header.clientWidth,
      pillLeft:pill.left,pillRight:pill.right,
      chatLeft:chat.left,chatRight:chat.right,
      popLeft:pop.left,popRight:pop.right,popTop:pop.top,popBottom:pop.bottom,
      vw:window.innerWidth,vh:window.innerHeight,
    };
  });
  assert.equal(lib360.headerOverflow,false,'Library header must not overflow at 360px');
  assert(lib360.pillLeft>=0&&lib360.pillRight<=lib360.vw,'Voice split pill fits inside 360px viewport');
  assert(lib360.chatLeft>=0&&lib360.chatRight<=lib360.vw,'Chat toggle fits inside 360px viewport');
  assert(lib360.pillRight<=lib360.chatLeft,'Voice pill and chat toggle do not overlap');
  assert(lib360.popLeft>=12&&lib360.popRight<=lib360.vw-12,'Library voice popover stays within 12px horizontal margins at 360px');
  assert(lib360.popTop>=12&&lib360.popBottom<=lib360.vh-12,'Library voice popover stays within 12px vertical margins at 360px');
  await river.page.screenshot({path:path.join(root,'voice-library-360.png')});
  await river.page.keyboard.press('Escape');
  await river.page.locator('#voice-mixer').waitFor({state:'hidden'});

  // Real 360x800 game-player header check while muted: red mic-slash badge visible, no overflow.
  const launch=await river.page.evaluate(async()=>{
    const r=await fetch('/api/apps/pond/launch',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({mode:'solo'})});
    return {ok:r.ok,body:await r.json()};
  });
  assert.equal(launch.ok,true,'launch solo fixture game');
  const gamePage=await river.context.newPage();
  gamePage.on('pageerror',e=>console.error('game',e.message));
  await gamePage.setViewportSize({width:360,height:800});
  await gamePage.goto(launch.body.url);
  await gamePage.locator('#voice-levels').waitFor({state:'visible'});
  // Mute from the game bar popover and verify the Library confirms it back to the game bar badge.
  await gamePage.locator('#voice-levels').click();
  await gamePage.locator('#game-voice-mixer').waitFor({state:'visible'});
  const gamePop360=await gamePage.evaluate(()=>{
    const pop=document.querySelector('#game-voice-mixer').getBoundingClientRect();
    return {left:pop.left,right:pop.right,top:pop.top,bottom:pop.bottom,vw:window.innerWidth,vh:window.innerHeight};
  });
  assert(gamePop360.left>=10&&gamePop360.right<=gamePop360.vw-10,'Game voice popover stays inside 10px margins at 360px');
  await gamePage.locator('#game-voice-mixer').getByRole('button',{name:'Mute',exact:true}).click();
  await gamePage.waitForFunction(()=>document.querySelector('#voice-levels')?.dataset.muted==='true'&&!document.querySelector('.voice-split-badge')?.hidden);
  await gamePage.screenshot({path:path.join(root,'voice-game-popover-360.png')});
  await gamePage.keyboard.press('Escape');
  await gamePage.locator('#game-voice-mixer').waitFor({state:'hidden'});
  const gameBar360=await gamePage.evaluate(()=>{
    const bar=document.querySelector('#player-header .bar');
    const pill=document.querySelector('#voice-levels').getBoundingClientRect();
    const badge=document.querySelector('.voice-split-badge').getBoundingClientRect();
    return {
      overflow:bar.scrollWidth>bar.clientWidth,
      pillLeft:pill.left,pillRight:pill.right,pillWidth:pill.width,pillHeight:pill.height,
      badgeWidth:badge.width,badgeHeight:badge.height,
      vw:window.innerWidth,
    };
  });
  assert.equal(gameBar360.overflow,false,'Game bar must not overflow at 360px');
  assert(gameBar360.pillLeft>=0&&gameBar360.pillRight<=gameBar360.vw,'Game voice pill fits inside 360px viewport');
  assert(gameBar360.pillWidth>=40&&gameBar360.pillHeight>=40,'Game voice pill target is at least 40x40px');
  assert(gameBar360.badgeWidth>=14&&gameBar360.badgeHeight>=14,'Muted badge is visible and at least 14px');
  await gamePage.screenshot({path:path.join(root,'voice-game-muted-360.png')});
  // Unmute again before continuing to observer-tab checks.
  await gamePage.locator('#voice-levels').click();
  await gamePage.locator('#game-voice-mixer').getByRole('button',{name:'Unmute',exact:true}).click();
  await gamePage.waitForFunction(()=>document.querySelector('#voice-levels')?.dataset.muted==='false');
  await gamePage.close();
  await river.page.setViewportSize({width:1280,height:800});
  console.log('360px Library and Game Player split-pill checks passed.');
  // A second tab sees the connection but never opens another microphone.
  const tab=await river.context.newPage();await tab.goto(origin+'/library');
  await tab.waitForFunction(()=>document.querySelector('#voice-toggle')?.dataset.state==='elsewhere');
  assert.equal(await tab.evaluate(()=>window.connections.length),0);
  assert.equal(await tab.evaluate(()=>window.voiceNotes.length),0,'observer tabs never chime');
  const observerTab=await river.context.newPage();await observerTab.goto(origin+'/library');
  await observerTab.waitForFunction(()=>document.querySelector('#voice-toggle')?.dataset.state==='elsewhere');
  await observerTab.close();await sleep(3500);
  assert.equal(await river.page.locator('#voice-toggle').getAttribute('data-state'),'on','closing a non-owning tab must not stop voice');
  await tab.locator('#voice-toggle').click();
  await river.page.waitForFunction(()=>document.querySelector('#voice-toggle').dataset.state==='off');
  console.log('Second-tab status and remote off passed.');
  await river.page.locator('#voice-toggle').click();await river.page.waitForFunction(()=>document.querySelector('#voice-toggle').dataset.state==='on');
  const visitor=await player('visitor','visit'),other=await player('other');
  await sleep(4100);const beforeJoin=await river.page.evaluate(()=>window.voiceNotes.length);
  await visitor.page.locator('#voice-toggle').click();await other.page.locator('#voice-toggle').click();
  await visitor.page.waitForFunction(()=>document.querySelector('#voice-toggle').dataset.state==='on');
  await other.page.waitForFunction(()=>document.querySelector('#voice-toggle').dataset.state==='on');
  await visitor.page.waitForFunction(()=>document.querySelectorAll('.voice-badge').length===3);
  await other.page.waitForFunction(()=>document.querySelectorAll('.voice-badge').length===1);
  assert.equal(await river.page.evaluate(()=>window.voiceNotes.length),beforeJoin+3,'one melody for another participant joining, none for the other group');
  await sleep(4100);
  await fetch(config.api+'/fixture/end-visit',{method:'POST'});
  await visitor.page.waitForFunction(()=>window.connections.every(pc=>pc.connectionState==='closed'),{},{timeout:10000});
  await river.page.waitForFunction(n=>window.voiceNotes.length===n,beforeJoin+6);
  const exitNotes=await river.page.evaluate(()=>window.voiceNotes.slice(-3));assert(exitNotes[0]>exitNotes[1]&&exitNotes[1]>exitNotes[2]);
  console.log('Visit cutoff removed live media; unrelated group remained isolated.');
  const cleanup=await fetch(config.api+'/fixture/orphan-cleanup',{method:'POST'});assert.equal(cleanup.status,200);
  for(const {page} of [river,sky,other])await page.waitForFunction(()=>window.connections.every(pc=>pc.connectionState==='closed'),{},{timeout:15000});
  console.log('Real sidecar orphan-room cleanup cut off remaining media.');
  await river.page.waitForFunction(()=>window.connections.every(pc=>pc.connectionState==='closed'));
  for(const viewport of [{width:390,height:844},{width:1280,height:800}]){
    await river.page.setViewportSize(viewport);
    const [voice,chat]=await Promise.all(['.voice-split','#chat-toggle'].map(s=>river.page.locator(s).boundingBox()));
    assert(Math.abs(voice.y-chat.y)<10,'voice stays beside text chat');assert(voice.x+voice.width<=chat.x+1,'controls do not overlap');
  }
  passed=true;console.log('Voice UI and real-media journey passed. Artifacts: '+root);
}finally{
  await browser?.close();if(proxy){proxy.closeAllConnections();await new Promise(r=>proxy.close(r));}
  try{process.kill(-fixture.pid,'SIGKILL');}catch{}
  fs.closeSync(log);if(!passed)console.error('Voice failure artifacts: '+root);
}
