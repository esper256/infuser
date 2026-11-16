// Responsive layout regression using real CSS and chat code with a mock API.
// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs node tests/chat_layout_journey.mjs
// Set INFUSER_BROWSER=webkit for desktop WebKit; this is not an iOS device test.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const engine=process.env.INFUSER_BROWSER||'chromium';
const playwright=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await playwright[engine].launch({headless:true,...(engine==='chromium'&&process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
const messages=Array.from({length:30},(_,i)=>({id:i+1,author:{id:'friend',name:'OrbitFox'},text:`Message ${i+1}: history to read.`,created:Date.now()/1000,can_delete:false}));
const fixture=`<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><link rel="stylesheet" href="/assets/clubhouse.css"><link rel="stylesheet" href="/assets/themes/nightglass.css"><link rel="stylesheet" href="/assets/themes/cartridge-club.css"></head><body>
<style>@media(max-width:650px),(pointer:coarse) and (max-height:500px){header{padding-top:180px}}</style><header><div id="friends"><span data-player-id="friend">OrbitFox</span></div></header>
<script type="module">
import {initChat} from '/assets/chat.js';
initChat({me:{player_id:'me'},api:async(path,options)=>{const response=await fetch(path,options);return response.json();},profileAvatar:()=>{const avatar=document.createElement('span');avatar.className='avatar';return avatar;}});
</script></body></html>`;
try {
 for(const theme of ['nightglass','cartridge-club']){
  for(const size of [{width:1280,height:900},{width:390,height:844},{width:320,height:568},{width:844,height:390}]){
   const mobile=size.width<1000;
   const context=await browser.newContext({viewport:size,hasTouch:mobile,isMobile:mobile});
   await context.route('https://chat.test/**',async route=>{
    const path=new URL(route.request().url()).pathname;
    if(path==='/api/chat')return route.fulfill({json:{messages}});
    if(path.startsWith('/assets/'))return route.fulfill({body:await readFile(new URL('../web/'+path.slice(8),import.meta.url)),contentType:path.endsWith('.js')?'text/javascript':'text/css'});
    return route.fulfill({body:fixture,contentType:'text/html'});
   });
   const page=await context.newPage(),errors=[];
   page.on('pageerror',e=>errors.push(e.message));
   await page.goto('https://chat.test/');
   await page.evaluate(t=>document.documentElement.dataset.theme=t,theme);
   await page.waitForFunction(()=>document.querySelectorAll('.chat-message').length===30);
   await page.locator('#chat-toggle').click();
   const settle=()=>page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
   await settle();
   async function geometry(top,height,minLog){
    const panel=await page.locator('#chat-panel').boundingBox();
    const log=await page.locator('#chat-log').boundingBox();
    const send=await page.locator('#chat-send').boundingBox();
    assert(panel.x>=11&&panel.x+panel.width<=size.width-11,'panel fits viewport width');
    assert(panel.y>=top+11&&panel.y+panel.height<=top+height-11,'panel fits visible viewport height');
    assert(log.height>=minLog,`${engine} ${theme} ${size.width}: message area ${log.height}px < ${minLog}px`);
    assert(send.y>=panel.y&&send.y+send.height<=panel.y+panel.height,'composer stays inside panel');
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'no horizontal overflow');
    return {panel,log};
   }
   const initial=await geometry(0,size.height,mobile?180:350);
   if(mobile){
    assert.equal(initial.panel.y,12,'mobile panel uses viewport top despite wrapped header');
    assert.equal(await page.evaluate(()=>document.activeElement.id),'chat-log','reading does not automatically summon the keyboard');
    assert.equal(await page.locator('.chat-pointer').count(),0);
    await page.locator('#chat-text').fill('Keep my draft');
    // Synthetic VisualViewport shrink + pan models keyboard/browser chrome events.
    // Neither mobile emulation nor desktop WebKit opens a real iOS keyboard.
    await page.evaluate(()=>{
     const viewport=new EventTarget();
     Object.assign(viewport,{offsetTop:48,height:360});
     window.testViewport=viewport;
     // Relay to the native object whose listeners initChat registered.
     const native=window.visualViewport;
     window.panTestViewport=()=>native.dispatchEvent(new Event('scroll'));
     Object.defineProperty(window,'visualViewport',{value:viewport,configurable:true});
     native.dispatchEvent(new Event('resize'));
    });
    await settle();
    const keyboard=await geometry(48,360,130);
    console.log(`  synthetic keyboard viewport: log ${keyboard.log.height}px`);
    const log=page.locator('#chat-log');
    await log.evaluate(e=>e.scrollTop-=80);await settle();
    const scroll=await log.evaluate(e=>e.scrollTop);
    await page.evaluate(()=>{window.testViewport.offsetTop=72;window.panTestViewport();});
    await settle();await geometry(72,360,130);
    assert(Math.abs(await log.evaluate(e=>e.scrollTop)-scroll)<=1,'viewport pan preserves history position');
    assert.equal(await page.locator('#chat-text').inputValue(),'Keep my draft');
    await page.evaluate(()=>{delete window.visualViewport;window.dispatchEvent(new Event('resize'));});
    await settle();await geometry(0,size.height,180);
    if(size.width===390){
     await page.setViewportSize({width:844,height:390});await settle();
     const rotated=await page.locator('#chat-panel').boundingBox();
     assert.equal(rotated.y,12,'rotation keeps the touch layout');
     assert(rotated.y+rotated.height<=379,'landscape fits the viewport');
     assert((await page.locator('#chat-log').boundingBox()).height>=180);
     await page.setViewportSize(size);await settle();
     await geometry(0,size.height,180);
    }
   }else{
    assert.equal(initial.panel.width,437);
    assert.equal(initial.panel.height,672);
    assert(initial.panel.y>12,'desktop remains badge anchored');
    assert.equal(await page.evaluate(()=>document.activeElement.id),'chat-text');
   }
   await page.locator('#chat-close').click();
   assert(await page.locator('#chat-panel').isHidden());
   assert.equal(await page.evaluate(()=>document.activeElement.id),'chat-toggle');
   assert.deepEqual(errors,[]);
   console.log(`PASS ${engine} ${theme} ${size.width}x${size.height}: log ${initial.log.height}px`);
   await context.close();
  }
 }
} finally {await browser.close();}
