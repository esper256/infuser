// Real browser regression for small upward gestures immediately after opening chat.
// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs INFUSER_CHROMIUM=/path/to/chromium node tests/chat_scroll_journey.mjs
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.INFUSER_CHROMIUM});
const author={id:'friend',name:'OrbitFox',avatar:null};
let messages=Array.from({length:30},(_,i)=>({id:i+1,author,text:`Message ${i+1}: a little history to scroll through.`,created:Date.now()/1000-60+i,can_delete:false}));
const fixture=`<!doctype html><html><head><link rel="stylesheet" href="/assets/clubhouse.css"><link rel="stylesheet" href="/assets/themes/nightglass.css"><link rel="stylesheet" href="/assets/themes/cartridge-club.css"></head><body>
<header><div id="friends"><span data-player-id="friend">OrbitFox</span></div></header>
<script type="module">
import {initChat} from '/assets/chat.js';
initChat({me:{player_id:'me'},api:async(path,options)=>{const response=await fetch(path,options);return response.json();},profileAvatar:()=>{const avatar=document.createElement('span');avatar.className='avatar';avatar.textContent='OF';return avatar;}});
</script></body></html>`;
try {
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.route('https://chat.test/**',async route=>{
  const path=new URL(route.request().url()).pathname;
  if(path==='/api/chat')return route.fulfill({json:{messages}});
  if(path.startsWith('/assets/'))return route.fulfill({body:await readFile(new URL('../web/'+path.slice(8),import.meta.url)),contentType:path.endsWith('.js')?'text/javascript':'text/css'});
  return route.fulfill({body:fixture,contentType:'text/html'});
 });
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto('https://chat.test/');
 await page.waitForFunction(()=>document.querySelectorAll('.chat-message').length===30);
 const gap=()=>page.locator('#chat-log').evaluate(e=>e.scrollHeight-e.clientHeight-e.scrollTop);
 const settle=()=>page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
 async function refresh(){
  const response=page.waitForResponse(r=>new URL(r.url()).pathname==='/api/chat');
  await page.evaluate(()=>window.dispatchEvent(new Event('online')));
  await (await response).finished();
  await page.waitForFunction(count=>document.querySelectorAll('.chat-message').length===count,messages.length);
  await settle();
 }
 for(const theme of ['nightglass','cartridge-club']){
  await page.evaluate(t=>document.documentElement.dataset.theme=t,theme);
  for(const width of [1280,390,320]){
   await page.setViewportSize({width,height:width===1280?900:844});
   await page.locator('#chat-toggle').click();await settle();
   assert(await gap()<=1,'opening starts at latest');
   const log=page.locator('#chat-log');await log.hover();
   await page.mouse.wheel(0,-1);await page.waitForTimeout(70);
   await refresh();assert(await gap()>=1,'even a one-pixel upward gesture survives unchanged polling');
   // High-resolution trackpads often start with a gesture smaller than 32 CSS px.
   for(let i=0;i<4;i++){
    await page.mouse.wheel(0,-8);await page.waitForTimeout(70);
    assert(await gap()>=7*(i+1),`${theme} ${width}: upward wheel ${i+1} was pulled back to latest`);
   }
   // A minimal scroll must also survive a refresh and presence repositioning.
   await log.evaluate(e=>e.scrollTop=e.scrollHeight-e.clientHeight-8);await settle();
   const top=await log.evaluate(e=>e.scrollTop);
   await refresh();
   await page.evaluate(()=>document.querySelector('#friends').replaceChildren(document.querySelector('#friends').firstElementChild.cloneNode(true)));
   await settle();assert(Math.abs(await log.evaluate(e=>e.scrollTop)-top)<=1,'unchanged polling/presence preserves a small upward scroll');
   messages.push({id:messages.length+1,author,text:'New while reading earlier messages',created:Date.now()/1000,can_delete:false});
   await refresh();await page.locator('#chat-new').waitFor();
   assert(Math.abs(await log.evaluate(e=>e.scrollTop)-top)<=1,'new messages preserve history position');
   await page.locator('#chat-new').click();await settle();assert(await gap()<=1,'explicit latest returns to bottom');
   messages.push({id:messages.length+1,author,text:'Follow this at the bottom',created:Date.now()/1000,can_delete:false});
   await refresh();assert(await gap()<=1,'new messages still follow at bottom');
   await log.focus();await page.keyboard.press('ArrowUp');await page.waitForTimeout(150);assert(await gap()>1,'keyboard scroll moves into history');
   await page.locator('#chat-close').click();
  }
 }
 assert.deepEqual(errors,[]);
 console.log('PASS: small upward wheels, keyboard, polling, presence changes, new-message anchoring and explicit latest; both themes at 1280/390/320px.');
} finally {await browser.close();}
