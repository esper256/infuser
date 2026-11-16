// Responsive layout regression using real CSS and chat code with a mock API.
// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs node tests/chat_layout_journey.mjs
// Set INFUSER_BROWSER=webkit for desktop WebKit; this is not an iOS device test.
import assert from 'node:assert/strict';
import {readFile,mkdir} from 'node:fs/promises';
const engine=process.env.INFUSER_BROWSER||'chromium';
const playwright=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await playwright[engine].launch({headless:true,...(engine==='chromium'&&process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
const messages=Array.from({length:30},(_,i)=>({id:i+1,author:{id:'friend',name:'OrbitFox'},text:`Message ${i+1}: history to read.`,created:Date.now()/1000,can_delete:false}));
const fixture=`<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><link rel="stylesheet" href="/assets/clubhouse.css"><link rel="stylesheet" href="/assets/themes/nightglass.css"><link rel="stylesheet" href="/assets/themes/cartridge-club.css"></head><body>
<style>@media(max-width:650px),(pointer:coarse) and (max-height:500px){header{padding-top:180px}}</style><header><div id="friends"><span data-player-id="friend">OrbitFox</span></div></header>
<script type="module">
import {initChat} from '/assets/chat.js';
initChat({me:{player_id:'me'},api:async(path,options)=>{const response=await fetch(path,options);const body=await response.json();if(!response.ok)throw new Error(body.error);return body;},profileAvatar:()=>{const avatar=document.createElement('span');avatar.className='avatar';return avatar;}});
</script></body></html>`;
try {
 for(const theme of ['nightglass','cartridge-club']){
  for(const size of [{width:1280,height:900},{width:390,height:844},{width:320,height:568},{width:844,height:390}]){
   const mobile=size.width<1000;
   const context=await browser.newContext({viewport:size,hasTouch:mobile,isMobile:mobile});
   let sent=[],heldSend;
   await context.route('https://chat.test/**',async route=>{
    const path=new URL(route.request().url()).pathname;
    if(path==='/api/chat'){
     if(route.request().method()==='POST'){sent.push(route.request().postDataJSON());heldSend=route;return;}
     return route.fulfill({json:{messages}});
    }
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
   const initial=await geometry(0,size.height,mobile?240:580);
   assert.equal(await page.locator('.chat-heading,.chat-audience').count(),0,'no redundant header');
   const input=page.locator('#chat-text'),send=page.locator('#chat-send'),count=page.locator('#chat-limit');
   assert(await count.isHidden());assert.equal(await send.getAttribute('aria-label'),'Send message');
   const arrow=await send.locator('svg').boundingBox(),button=await send.boundingBox(),logBefore=await page.locator('#chat-log').boundingBox();
   await input.fill('x'.repeat(239));assert(await count.isHidden());
   await input.fill('x'.repeat(240));assert(await count.isVisible());assert.equal(await count.textContent(),'240/280');
   assert.deepEqual(await send.locator('svg').boundingBox(),arrow,'count never shifts the arrow');
   assert.deepEqual(await send.boundingBox(),button,'count never resizes the send button');
   assert.deepEqual(await page.locator('#chat-log').boundingBox(),logBefore,'count never reduces history space');
   assert(await input.evaluate(e=>{const b=document.querySelector('#chat-send').getBoundingClientRect(),r=e.getBoundingClientRect();return r.right-parseFloat(getComputedStyle(e).paddingRight)<=b.left;}),'typed text stays clear of the inset button');
   await input.fill('');assert(await count.isHidden());assert(await send.isDisabled());
   // Return to the reading focus used when opening on touch screens.
   if(mobile)await page.locator('#chat-log').focus();
   await mkdir(new URL('../.local/chat-compact/',import.meta.url),{recursive:true});
   await page.screenshot({path:new URL(`../.local/chat-compact/${theme}-${size.width}.png`,import.meta.url).pathname});
   if(!mobile){
    await input.fill('A draft with\na second line');await input.press('Shift+Enter');
    assert.equal(sent.length,0,'Shift+Enter does not submit');
    const draft=await input.inputValue();
    await send.click();await page.waitForFunction(()=>document.querySelector('#chat-send').getAttribute('aria-busy')==='true');
    assert(await send.isDisabled());assert(await input.evaluate(e=>e.readOnly));
    assert.deepEqual(await send.locator('svg').boundingBox(),arrow,'sending preserves the arrow');
    await heldSend.fulfill({status:503,json:{error:'Please retry.'}});
    await page.locator('#chat-error').getByText('Please retry.',{exact:true}).waitFor();
    assert.equal(await input.inputValue(),draft,'failure retains the draft');
    assert.equal(await send.getAttribute('aria-label'),'Send message');
    await input.press('Enter');await page.waitForFunction(()=>document.querySelector('#chat-send').getAttribute('aria-busy')==='true');
    assert.equal(sent.length,2);assert.equal(sent[0].client_id,sent[1].client_id,'retry reuses the message ID');
    await heldSend.fulfill({json:{id:31}});await page.waitForFunction(()=>!document.querySelector('#library-chat').hasAttribute('data-sending'));
    assert.equal(await input.inputValue(),'');assert(await count.isHidden());assert(await send.isDisabled());
    assert.equal(await send.locator('svg').count(),1,'successful send retains the icon');
    await page.mouse.click(size.width-5,size.height-5);assert(await page.locator('#chat-panel').isHidden(),'desktop outside click closes chat');
    await page.locator('#chat-toggle').click();await settle();
   }
   if(mobile){
    assert.equal(initial.panel.y,12,'mobile panel uses viewport top despite wrapped header');
    assert.equal(await page.evaluate(()=>document.activeElement.id),'chat-log','reading does not automatically summon the keyboard');
    assert.equal(await page.locator('.chat-pointer').count(),0);
    const close=await page.locator('#chat-close').boundingBox(),field=await input.boundingBox();
    assert(close.y<initial.panel.y+20&&close.x>initial.panel.x+initial.panel.width-50,'mobile close floats at upper right');
    assert(field.width>=initial.panel.width-28,'composer uses the full width');
    await page.locator('#chat-log').evaluate(e=>e.scrollTop-=100);
    assert.deepEqual(await page.locator('#chat-close').boundingBox(),close,'floating close stays put while history scrolls');
    await page.locator('#chat-log').evaluate(e=>e.scrollTop=e.scrollHeight);

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
    if(size.width===844){
     await page.evaluate(()=>{
      window.testViewport.height=280;
      document.querySelector('#chat-connection').textContent='Reconnecting…';
      document.querySelector('#chat-error').textContent='Message failed. Try again.';
      document.querySelector('#chat-new').hidden=false;
      window.dispatchEvent(new Event('resize'));
     });
     await settle();
     const short=await geometry(72,280,50);
     console.log(`  short landscape with both status bars and latest button: log ${short.log.height}px`);
     await page.locator('#chat-text').fill('Draft in landscape');
     assert(await page.locator('#chat-send').isEnabled());
     for(const id of ['chat-close','chat-text','chat-send','chat-new','chat-error','chat-connection']){
      const box=await page.locator('#'+id).boundingBox();
      assert(box.y>=84&&box.y+box.height<=341,`${id} remains inside short viewport`);
     }
     await page.evaluate(()=>{
      document.querySelector('#chat-connection').textContent='';
      document.querySelector('#chat-error').textContent='';
      document.querySelector('#chat-new').hidden=true;
     });
    }
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
   if(mobile)await page.locator('#chat-close').click();
   else{
    assert(await page.locator('#chat-close').isHidden(),'desktop has no close chrome');
    await page.locator('#chat-log').press('Escape');
   }
   assert(await page.locator('#chat-panel').isHidden());
   assert.equal(await page.evaluate(()=>document.activeElement.id),'chat-toggle');
   assert.deepEqual(errors,[]);
   console.log(`PASS ${engine} ${theme} ${size.width}x${size.height}: log ${initial.log.height}px`);
   await context.close();
  }
 }
} finally {await browser.close();}
