// Visible chat authors, animated presence ordering and synchronized tails; real UI, mocked API.
// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs node tests/chat_speakers_journey.mjs
// Set INFUSER_TOUCH=1 to also exercise touch opening and coarse-pointer styles.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const root=new URL('../',import.meta.url);
const browser=await chromium.launch({headless:true,...(process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
const context=await browser.newContext({hasTouch:process.env.INFUSER_TOUCH==='1'});const page=await context.newPage();
const errors=[];page.on('pageerror',e=>errors.push(e.message));
const me={player_id:'self',name:'Alex',admin:false,avatar:'circuit-fox',avatars:[{id:'circuit-fox',name:'Circuit Fox'},{id:'moss-knight',name:'Moss Knight'}]};
let friends=Array.from('abcdefghij',(id,i)=>({id,name:'Friend '+id.toUpperCase(),avatar:i%2?'moss-knight':'circuit-fox'}));
let messages=Array.from({length:40},(_,i)=>({id:i+1,author:friends[i<20?4:9],text:'Message '+i+' from the older conversation. Enough words to make reading and scrolling realistic.',created:Date.now()/1000-86400+i,can_delete:false}));
let launches=[];let stale=false;
friends[0].join_targets=[{app_id:'firefly',title:'Firefly Drift',session_id:'session-a',player_id:'a'}];
friends[1].join_targets=[{app_id:'firefly',title:'Firefly Drift',session_id:'session-b',player_id:'b'}];
let tagCatalog=['Arcade','Chill','Cooperative','Adventure','Puzzle','Racing','Strategy','Something with a very long tag label'].map((label,i)=>({id:'tag-'+i,label,count:1}));
const game={id:'firefly',title:'Firefly Drift',card_art_url:'/art',play_modes:['solo'],tags:tagCatalog.map(t=>t.id),can_edit_tags:true,players:[],reactions:[],updated:1};
await context.route('https://library.test/**',async route=>{
 const path=new URL(route.request().url()).pathname;
 if(path==='/api/apps/firefly/launch'){launches.push(route.request().postDataJSON());return stale?route.fulfill({status:409,json:{error:'That session ended. Try again.'}}):route.fulfill({json:{state:'ready',launch_id:'launch',url:'https://library.test/playing'}});}
 if(path==='/playing')return route.fulfill({body:'Playing'});
 if(path==='/api/me')return route.fulfill({json:me});
 if(path==='/api/library')return route.fulfill({json:{apps:[game],friends,tags:tagCatalog}});
 if(path==='/api/chat')return route.fulfill({json:{messages}});
 if(path==='/api/client-state')return route.fulfill({json:{game_open:true,version:'test'}});
 if(path==='/api/profile'){me.avatar=route.request().postDataJSON().avatar;return route.fulfill({json:{avatar:me.avatar}});}
 let file=path==='/library'?'web/index.html':path==='/art'?'docs/design-explorations/card-art-guide/firefly-drift.webp':path.startsWith('/assets/')?'web/'+path.slice(8):null;
 const avatar=/^\/assets\/avatars\/([^/]+)\/(\d+)\.webp$/.exec(path);if(avatar)file=`assets/avatars/${avatar[1]}-${avatar[2]}.webp`;
 if(file)try{
  let body=await fs.readFile(new URL(file,root));
  if(file==='web/clubhouse.js')body=body.toString()+'\nwindow.refreshLibrary=refresh;';
  return route.fulfill({body,contentType:file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':file.endsWith('.webp')?'image/webp':'text/html'});
 }catch{}
 return route.fulfill({status:404,body:''});
});

await page.addInitScript(()=>{
 const raf=requestAnimationFrame;window.rafCallbacks=0;window.requestAnimationFrame=callback=>raf(time=>{rafCallbacks++;callback(time);});
});
try{
 await page.setViewportSize({width:1500,height:1000});await page.goto('https://library.test/library');await page.waitForFunction(()=>window.refreshLibrary);
 await page.waitForFunction(()=>document.querySelectorAll('.chat-message').length===40);
 const log=page.locator('#chat-log'),order=()=>page.locator('#friends [data-player-id]').evaluateAll(nodes=>nodes.map(n=>n.dataset.playerId));
 const pause=ms=>page.waitForTimeout(ms);
 const firstFriend=()=>page.locator('#friends .presence-person:not(.presence-self)[data-player-id]').first().getAttribute('data-player-id');
 async function sampleMovement(id,open=false,top=0){
  return page.evaluate(async({id,open,top})=>{
   const samples=[],log=document.querySelector('#chat-log');
   const capture=()=>{
    const badge=document.querySelector(`#friends [data-player-id="${id}"]`),animation=badge?.getAnimations().find(a=>a.id==='presence-reorder');
    const pointer=document.querySelector(`#chat-pointers [data-player-id="${id}"]`),rect=badge?.getBoundingClientRect();
    samples.push({x:rect?.x,time:animation?.currentTime,pointer:!!pointer,tip:pointer?pointer.getBoundingClientRect().left+pointer.getBoundingClientRect().width/2:null,center:rect?rect.left+rect.width/2:null});
   };
   capture();
   if(open)document.querySelector('#chat-toggle').click();else{log.scrollTop=top;log.dispatchEvent(new Event('scroll'));}
   for(let i=0;i<36;i++){await new Promise(requestAnimationFrame);capture();}
   return samples;
  },{id,open,top});
 }
 function verifyMotion(samples){
  const moving=samples.filter(s=>s.time!=null);
  assert(moving.some(s=>s.time>0&&s.time<200),'actual intermediate motion frames exist');
  assert(moving.filter(s=>s.time<250).every(s=>!s.pointer),'tail stays shut during acceleration and fast travel');
  assert(moving.some(s=>s.time>=280&&s.pointer),'tail opens during the decelerating part of motion');
  assert(samples.at(-1).pointer,'tail remains after settling');
  const distance=Math.abs(samples.at(-1).x-samples[0].x);
  assert(distance>20,'speaker moved left substantially');
  assert(Math.abs(samples[1].x-samples[0].x)<distance*.2,'first frame must not teleport');
  for(const s of samples.filter(s=>s.pointer))assert(Math.abs(s.tip-s.center)<2,'tail tip follows the avatar while it settles');
 }
 verifyMotion(await sampleMovement('j',true));assert.equal(await firstFriend(),'j');
 assert.equal(await page.locator('#chat-pointers .chat-pointer').count(),1,'old messages in view get tails, not only recent messages');
 const beforeOpenBounds=await page.locator('#chat-panel').boundingBox();
 verifyMotion(await sampleMovement('e',false,0));assert.equal(await firstFriend(),'e');
 await fs.mkdir(new URL('.local/chat-speakers/',root),{recursive:true});await page.screenshot({path:new URL('.local/chat-speakers/desktop.png',root).pathname});
 assert.equal(await log.evaluate(e=>e.scrollTop),0,'reordering does not pull history to the bottom');
 assert.deepEqual(await page.locator('#chat-panel').boundingBox(),beforeOpenBounds,'sorting must not move the chat panel');
 // Every author with even a partially visible message receives one tail.
 await log.evaluate(e=>{const row=e.querySelectorAll('.chat-message')[18];e.scrollTop+=row.getBoundingClientRect().top-e.getBoundingClientRect().top;});await pause(500);
 assert.deepEqual(await page.locator('#chat-pointers .chat-pointer').evaluateAll(nodes=>nodes.map(n=>n.dataset.playerId).sort()),['e','j']);
 assert.deepEqual((await order()).slice(1,3),['e','j']);
 await log.evaluate(e=>e.scrollTop=0);await pause(500);
 // Interrupt a movement twice; start each new motion from its current visual location.
 await log.evaluate(e=>{e.scrollTop=e.scrollHeight;});await pause(90);
 const interrupted=await page.locator('#friends [data-player-id="e"]').boundingBox();
 await log.evaluate(e=>{e.scrollTop=0;});await pause(30);
 const continued=await page.locator('#friends [data-player-id="e"]').boundingBox();
 assert(Math.abs(continued.x-interrupted.x)<50,'interrupted movement remains continuous');await pause(500);
 // A poll while reading older messages preserves both priority and scroll position.
 const top=await log.evaluate(e=>e.scrollTop);await page.evaluate(()=>refreshLibrary());await pause(80);
 assert.equal(await firstFriend(),'e');assert.equal(await log.evaluate(e=>e.scrollTop),top);
 // No per-frame JavaScript or DOM work once motion has settled.
 await page.evaluate(()=>{window.chatMutations=0;window.rafCallbacks=0;window.chatObserver=new MutationObserver(records=>chatMutations+=records.length);chatObserver.observe(document.body,{childList:true,subtree:true,attributes:true});});
 await pause(250);assert.equal(await page.evaluate(()=>chatMutations),0);assert.equal(await page.evaluate(()=>rafCallbacks),0,'no animation loop survives at idle');await page.evaluate(()=>chatObserver.disconnect());
 await page.locator('#chat-close').click();await pause(500);
 assert.deepEqual(await order(),['self',...friends.map(f=>f.id)],'closing restores ordinary presence order');
 for(const theme of ['nightglass','cartridge-club'])for(const width of [650,390,320]){
  await page.evaluate(t=>InfuserThemes.select(t),theme);await page.setViewportSize({width,height:850});await pause(450);
  await page.locator('#chat-toggle').click();await pause(550);assert.equal(await firstFriend(),'j','visible speaker comes out of overflow');
  assert.equal(await page.locator('#chat-pointers .chat-pointer').count(),0,'compact chat keeps its full-height, tail-free layout');
  await log.evaluate(e=>e.scrollTop=0);await pause(550);assert.equal(await firstFriend(),'e');
  assert.equal(await page.locator('#chat-pointers .chat-pointer').count(),0);
  assert.equal(await page.locator('#chat-pointers [data-player-id="j"]').count(),0);
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no page overflow');
  if(width===390&&theme==='nightglass'){
   await fs.mkdir(new URL('.local/chat-speakers/',root),{recursive:true});
   await page.screenshot({path:new URL('.local/chat-speakers/mobile.png',root).pathname});
  }
  await page.locator('#chat-close').click();await pause(450);
 }
 await page.setViewportSize({width:1500,height:1000});await pause(100);await page.emulateMedia({reducedMotion:'reduce'});await page.locator('#chat-toggle').click();await pause(100);
 assert.equal(await firstFriend(),'j');assert(await page.locator('#chat-pointers [data-player-id="j"]').isVisible());
 assert.equal(await page.locator('#friends').evaluate(e=>e.getAnimations({subtree:true}).filter(a=>a.id==='presence-reorder').length),0,'reduced motion uses no avatar animation');
 // Offline authors remain in history but never invent an online badge/tail.
 friends=friends.filter(f=>f.id!=='j');await page.evaluate(()=>refreshLibrary());await pause(100);
 assert.equal(await page.locator('#chat-pointers [data-player-id="j"]').count(),0);
 assert.deepEqual(errors,[]);
 console.log('PASS: visible historical authors drive stable presence order; acceleration/deceleration and late aligned tails; interrupted motion, idle stability, polling, overflow at 650/390/320px in both themes, close reset, reduced motion and offline authors.');
}finally{await browser.close();}
