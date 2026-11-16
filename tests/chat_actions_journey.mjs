// Real pointer/keyboard gestures against mocked chat APIs; never deletes real messages.
import assert from 'node:assert/strict';
import {readFile,mkdir} from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.INFUSER_CHROMIUM});
const fixture=`<!doctype html><meta name="viewport" content="width=device-width,initial-scale=1"><link rel="stylesheet" href="/assets/clubhouse.css"><link rel="stylesheet" href="/assets/themes/nightglass.css"><link rel="stylesheet" href="/assets/themes/cartridge-club.css"><header><div id="friends">Players</div></header><script type="module">
import {initChat} from '/assets/chat.js';
initChat({me:{player_id:'me'},api:async(path,options)=>{const r=await fetch(path,options),data=await r.json();if(!r.ok)throw new Error(data.error);return data;},profileAvatar:()=>{const n=document.createElement('span');n.className='avatar';return n;}});</script>`;
try{
 for(const mobile of [false,true])for(const theme of ['nightglass','cartridge-club']){
  const context=await browser.newContext({viewport:mobile?{width:390,height:844}:{width:1280,height:900},hasTouch:mobile,isMobile:mobile});
  let messages=Array.from({length:15},(_,i)=>({id:i+1,author:{id:i===14?'me':'friend',name:i===14?'Alex':'OrbitFox'},text:'Message '+(i+1)+': enough history to scroll through.',created:Date.now()/1000,can_delete:i>=13}));
  let deletes=[],failDelete=false,heldDelete,holdDelete=false;
  await context.route('https://chat.test/**',async route=>{
   const path=new URL(route.request().url()).pathname;
   if(route.request().method()==='DELETE'){
    deletes.push(path);if(holdDelete){heldDelete=route;return;}
    if(failDelete)return route.fulfill({status:503,json:{error:'Try again.'}});
    messages=messages.filter(m=>path!=='/api/chat/'+m.id);return route.fulfill({json:{ok:true}});
   }
   if(path==='/api/chat')return route.fulfill({json:{messages}});
   if(path.startsWith('/assets/'))return route.fulfill({body:await readFile(new URL('../web/'+path.slice(8),import.meta.url)),contentType:path.endsWith('.js')?'text/javascript':'text/css'});
   return route.fulfill({body:fixture,contentType:'text/html'});
  });
  const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.goto('https://chat.test/');await page.evaluate(t=>document.documentElement.dataset.theme=t,theme);
  await page.waitForFunction(()=>document.querySelectorAll('.chat-message').length===15);await page.locator('#chat-toggle').click();
  const bubble=page.locator('.chat-message[data-id="14"] .chat-bubble'),remove=bubble.locator('.chat-remove'),menu=page.locator('#chat-actions');
  const pause=ms=>page.waitForTimeout(ms);
  const refresh=async()=>{const response=page.waitForResponse('**/api/chat');await page.evaluate(()=>window.dispatchEvent(new Event('online')));await response;await pause(30);};
  assert.equal(await page.locator('.chat-message[data-id="13"] .chat-remove').count(),0,'no action without server permission');
  if(!mobile){
   assert.equal(await remove.evaluate(e=>getComputedStyle(e).visibility),'hidden');
   const before=await bubble.boundingBox();await bubble.hover();
   assert.equal(await remove.evaluate(e=>getComputedStyle(e).visibility),'visible');assert(await remove.isDisabled());
   const button=await remove.boundingBox();await page.mouse.click(button.x+button.width/2,button.y+button.height/2);
   assert.equal(deletes.length,0,'immediate click cannot delete');
   await pause(180);assert(await remove.isDisabled(),'control stays inactive during delay');
   await page.mouse.move(1100,850);assert(await remove.isDisabled());
   assert.equal(await remove.evaluate(e=>getComputedStyle(e).visibility),'hidden','leaving hides and disarms');
   await bubble.hover();await pause(280);assert(await remove.isDisabled(),'reenter starts a fresh half second');
   await pause(250);assert(await remove.isEnabled());assert.deepEqual(await bubble.boundingBox(),before,'revealing does not reflow messages');
   failDelete=true;await remove.click();await page.locator('#chat-error').getByText('Try again.',{exact:true}).waitFor();
   assert.equal(await bubble.count(),1,'failed delete keeps the message');failDelete=false;
   await bubble.focus();await bubble.press('Shift+F10');await menu.waitFor();
   await page.keyboard.press('ArrowDown');assert.equal(await page.evaluate(()=>document.activeElement.id),'chat-cancel-action');
   await page.keyboard.press('Escape');assert(await menu.isHidden());assert(await page.locator('#chat-panel').isVisible());
   await bubble.press('Shift+F10');await page.locator('#chat-delete').click();
   await bubble.waitFor({state:'detached'});assert.equal(deletes.length,2);
  }else{
   assert.equal(await remove.evaluate(e=>getComputedStyle(e).display),'none','mobile has no inline delete controls');
   const cdp=await context.newCDPSession(page);
   const point=async()=>{const r=await bubble.boundingBox();return {x:Math.round(r.x+r.width/2),y:Math.round(r.y+r.height/2)};};
   const start=async p=>cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{...p,radiusX:2,radiusY:2}]});
   const end=()=>cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
   await bubble.tap();await pause(600);assert(await menu.isHidden());assert.equal(deletes.length,0,'short tap does nothing');
   let p=await point();await start(p);await pause(100);await cdp.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:p.x,y:p.y-35}]});await pause(550);await end();
   assert(await menu.isHidden(),'scroll gesture cancels hold');
   await page.locator('#chat-log').evaluate(e=>e.scrollTop=e.scrollHeight);await pause(100);
   p=await point();await start(p);await pause(620);await menu.waitFor();await end();await pause(80);
   assert.equal(deletes.length,0,'release after hold never activates Delete');
   const box=await menu.boundingBox();assert(box.x>=0&&box.x+box.width<=390&&box.y>=0&&box.y+box.height<=844);
   await mkdir(new URL('../.local/chat-actions/',import.meta.url),{recursive:true});await page.screenshot({path:new URL(`../.local/chat-actions/${theme}.png`,import.meta.url).pathname});
   await page.locator('#chat-cancel-action').tap();assert(await menu.isHidden());
   // A permission change closes an already-open menu before it can act.
   await start(await point());await pause(620);await end();await menu.waitFor();
   messages[13]={...messages[13],can_delete:false};await refresh();await menu.waitFor({state:'hidden'});assert.equal(deletes.length,0);
   messages[13]={...messages[13],can_delete:true};await refresh();
   await start(await point());await pause(620);await end();await menu.waitFor();
   holdDelete=true;await page.locator('#chat-delete').tap();await pause(30);assert.equal(deletes.length,1);
   await bubble.dispatchEvent('contextmenu');assert(await menu.isHidden(),'in-flight deletion cannot be submitted twice');
   messages=messages.filter(m=>m.id!==14);await heldDelete.fulfill({json:{ok:true}});await bubble.waitFor({state:'detached'});
  }
  // Closing chat dismisses an action menu; no pending reveal/hold survives it.
  const own=page.locator('.chat-message[data-id="15"] .chat-bubble');await own.focus();await own.press('Shift+F10');await menu.waitFor();
  await page.locator('#chat-toggle').evaluate(e=>e.click());assert(await menu.isHidden());await pause(600);assert(await menu.isHidden());
  assert.deepEqual(errors,[]);console.log(`PASS ${theme} ${mobile?'touch hold, cancellation, permissions and release safety':'hover delay, reset, keyboard actions and failure recovery'}`);
  await context.close();
 }
}finally{await browser.close();}
