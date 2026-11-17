// Native editing and hit targets; desktop Chromium emulation is not an iOS test.
import assert from 'node:assert/strict';
import {readFile,mkdir} from 'node:fs/promises';
const engine=process.env.INFUSER_BROWSER||'chromium';
const pw=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await pw[engine].launch({headless:true,...(engine==='chromium'?{ignoreDefaultArgs:['--hide-scrollbars'],executablePath:process.env.INFUSER_CHROMIUM}:{})});
const root=new URL('../',import.meta.url);
try{
 for(const theme of ['nightglass','cartridge-club'])for(const width of [1280,390,320,844]){
  const mobile=width<1000,context=await browser.newContext({viewport:{width,height:width===844?390:900},hasTouch:mobile,isMobile:mobile});let sends=0;
  await context.route('https://editing.test/**',async route=>{
   const path=new URL(route.request().url()).pathname;
   if(path==='/api/chat'){if(route.request().method()==='POST')sends++;return route.fulfill({json:{messages:[]}});}
   if(path.startsWith('/assets/'))return route.fulfill({body:await readFile(new URL('web/'+path.slice(8),root)),contentType:path.endsWith('.js')?'text/javascript':'text/css'});
   return route.fulfill({contentType:'text/html',body:`<meta name="viewport" content="width=device-width,initial-scale=1"><link rel="stylesheet" href="/assets/clubhouse.css"><link rel="stylesheet" href="/assets/themes/nightglass.css"><link rel="stylesheet" href="/assets/themes/cartridge-club.css"><html data-theme="${theme}"><header><div id="friends">Friends</div></header><script type="module">import {initChat} from '/assets/chat.js';initChat({me:{player_id:'me'},api:async(p,o)=>(await fetch(p,o)).json(),profileAvatar:()=>document.createElement('span')});</script>`});
  });
  const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));await page.goto('https://editing.test/');await page.locator('#chat-toggle').click();
  const input=page.locator('#chat-text'),scroll=page.locator('#chat-scrollbar'),send=page.locator('#chat-send');
  // A plain native LTR textarea is the reference for mixed-direction selection.
  await page.evaluate(()=>{const e=document.createElement('textarea');e.id='reference';e.style.cssText='position:fixed;top:0;left:0;width:1000px;height:80px;direction:ltr;unicode-bidi:plaintext';document.body.append(e);});
  for(const text of ['Hello, world! 123.','123! (456)?','Hello שלום 123!','שלום 123!']){
   const results=[];
   for(const field of [input,page.locator('#reference')]){
    await field.fill(text);await field.press('Home');await field.press('ArrowRight');await field.press('Shift+ArrowRight');
    const selection=await field.evaluate(e=>[e.selectionStart,e.selectionEnd,e.selectionDirection]);
    await field.press('Backspace');await field.pressSequentially('X');
    results.push({selection,value:await field.inputValue()});
   }
   assert.deepEqual(results[0],results[1],`native selection: ${text}`);
   if(text==='Hello, world! 123.')assert.deepEqual(results[0],{selection:[1,2,'forward'],value:'HXllo, world! 123.'});
  }
  await page.locator('#reference').evaluate(e=>e.remove());
  await input.fill('');await input.pressSequentially('English 123, שלום! '.repeat(12));
  assert(await scroll.evaluate(e=>e.scrollTop>0),'typing reveals the last line');
  await page.waitForFunction(()=>Math.abs(document.querySelector('#chat-scrollbar').scrollTop-document.querySelector('#chat-text').scrollTop)<=1);
  await scroll.evaluate(e=>e.scrollTop=0);await page.waitForFunction(()=>document.querySelector('#chat-text').scrollTop===0);
  const sb=await scroll.boundingBox();await page.mouse.move(sb.x+4,sb.y+sb.height/2);await page.mouse.wheel(0,80);
  await page.waitForFunction(()=>document.querySelector('#chat-text').scrollTop>0);
  await page.waitForFunction(()=>Math.abs(document.querySelector('#chat-scrollbar').scrollTop-document.querySelector('#chat-text').scrollTop)<=1);
  await input.press('Control+Home');await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));const first=await input.evaluate(e=>e.scrollTop);
  await input.press('Control+End');await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));assert(await input.evaluate(e=>e.scrollTop)>first+20,'native caret navigation scrolls the wrapper');
  await input.press('Shift+ArrowLeft');await input.press('Backspace');assert((await input.inputValue()).endsWith('שלום!'),'native selection edits the final character');
  await input.fill('x'.repeat(280));await input.press('Control+End');
  assert(await input.evaluate(e=>e.scrollHeight-e.clientHeight-e.scrollTop<24),'end caret stays visible after replacing text');
  await page.waitForFunction(()=>Math.abs(document.querySelector('#chat-scrollbar').scrollTop-document.querySelector('#chat-text').scrollTop)<=1);
  const before=await send.boundingBox();
  const target=await send.evaluate(e=>{const b=e.getBoundingClientRect();return {w:getComputedStyle(e,'::before').width,h:getComputedStyle(e,'::before').height,hits:[[b.x-3.5,b.y+18],[b.right+3.5,b.y+18],[b.x+18,b.y-3.5],[b.x+18,b.bottom+3.5]].map(([x,y])=>e.contains(document.elementFromPoint(x,y)))};});
  assert.deepEqual(target,{w:'44px',h:'44px',hits:[true,true,true,true]});
  await mkdir(new URL('.local/chat-editing/',root),{recursive:true});await page.screenshot({path:new URL(`.local/chat-editing/${theme}-${width}.png`,root).pathname});
  await page.mouse.click(before.x-3.5,before.y+18);await page.waitForFunction(()=>document.querySelector('#chat-text').value==='');assert.equal(sends,1,'expanded target submits');
  assert.deepEqual(await send.boundingBox(),before,'send geometry survives clearing');
  assert.deepEqual(errors,[]);console.log(`PASS native editing ${theme} ${width}`);await context.close();
 }
}finally{await browser.close();}
