// Real vendored picker under production CSP, with a local mock chat API.
import assert from 'node:assert/strict';
import {readFile,mkdir} from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await chromium.launch({headless:true,...(process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
const fixture=`<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><link rel="stylesheet" href="/assets/clubhouse.css"><link rel="stylesheet" href="/assets/themes/nightglass.css"><link rel="stylesheet" href="/assets/themes/cartridge-club.css"></head><body><header><div id="friends"><span data-player-id="friend">OrbitFox</span></div></header><script type="module" src="/fixture.js"></script></body></html>`;
const setup=`window.houseCallBursts=0;import {initChat} from '/assets/chat.js';initChat({me:{player_id:'me'},onHouseCall:()=>window.houseCallBursts++,api:async(path,options)=>{const r=await fetch(path,options);if(!r.ok)throw Error('Try again.');return r.json();},profileAvatar:()=>document.createElement('span')});`;
const textSamples=['Claude usage at 96%😨','Together 👨‍👩‍👧‍👦 👍🏽 🇺🇸 1️⃣ ❤️','Text 96% © ™ ♥︎ ✈︎','<img src=x onerror=alert(1)>😨','Hey🙂!'];
const history=textSamples.map((text,i)=>({id:i+1,author:{id:'friend',name:'Frizz'},text,created:Date.now()/1000,can_delete:false}));
try{
 for(const theme of ['nightglass','cartridge-club']){
  for(const width of [1280,390,320]){
   const context=await browser.newContext({viewport:{width,height:width===1280?900:700}});
   let posts=[],failData=false,houseCalls=[],messages=structuredClone(history),triggerCall=true;
   const requests=[],errors=[];
   await context.route('**/*',async route=>{
    const url=new URL(route.request().url()),path=url.pathname;
    requests.push(url.href);assert.equal(url.origin,'https://chat.test','no CDN requests');
    if(path==='/api/chat'){
     if(route.request().method()==='POST'){const sent=route.request().postDataJSON();posts.push(sent);if(sent.house_call){const call=houseCalls.find(call=>call.id===sent.house_call);messages.push({id:messages.at(-1).id+1,author:{id:'me',name:'Me'},text:'',created:Date.now()/1000,house_call:call,house_call_triggered:triggerCall});return route.fulfill({json:{id:messages.at(-1).id}});}return route.fulfill({status:503,json:{error:'Try again.'}});}
     return route.fulfill({json:{messages}});
    }
    if(path==='/api/house-calls')return route.fulfill({json:{images:houseCalls}});
    if(path==='/api/screenshots')return route.fulfill({json:{screenshots:[]}});
    if(path.startsWith('/assets/house-calls/'))return route.fulfill({body:await readFile(new URL('../web'+path,import.meta.url)),contentType:'image/png'});
    if(path==='/fixture.js')return route.fulfill({body:setup,contentType:'text/javascript'});
    if(path.startsWith('/assets/')){
     if(failData&&path.endsWith('/data.json'))return route.fulfill({status:503,body:''});
     let body=await readFile(new URL('../web/'+path.slice(8),import.meta.url));if(path==='/assets/chat.js')body=body.toString().replace('  poll();\n}', '  window.testChatPoll=poll;poll();\n}');
     return route.fulfill({body,contentType:path.endsWith('.js')?'text/javascript':path.endsWith('.json')?'application/json':'text/css'});
    }
    return route.fulfill({body:fixture,contentType:'text/html',headers:{'Content-Security-Policy':"default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"}});
   });
   const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
   await page.addInitScript(()=>{window.violations=[];document.addEventListener('securitypolicyviolation',e=>violations.push(e.violatedDirective));});
   await page.goto('https://chat.test');
   await page.evaluate(t=>document.documentElement.dataset.theme=t,theme);
   await page.locator('#chat-toggle').click();
   assert(!requests.some(url=>url.includes('/vendor/')),'picker assets load only on demand');
   await page.locator('.chat-message').last().waitFor();
   const paragraphs=page.locator('.chat-bubble p');
   assert.deepEqual(await paragraphs.allTextContents(),textSamples,'rendering preserves exact message text');
   assert.deepEqual(await paragraphs.nth(1).locator('.chat-inline-emoji').allTextContents(),['👨‍👩‍👧‍👦','👍🏽','🇺🇸','1️⃣','❤️'],'combined emoji remain whole');
   assert.equal(await paragraphs.nth(2).locator('span').count(),0,'numbers and text symbols keep normal typography');
   assert.equal(await paragraphs.nth(3).locator('img').count(),0,'message markup remains literal text');
   assert.equal(await paragraphs.nth(0).evaluate(e=>{const selection=getSelection(),range=document.createRange();range.selectNodeContents(e);selection.removeAllRanges();selection.addRange(range);const result=selection.toString();selection.removeAllRanges();return result;}),textSamples[0],'selection adds no synthetic spaces');
   const metrics=await paragraphs.last().evaluate(e=>{
    const emoji=e.querySelector('span'),style=getComputedStyle(emoji),base=getComputedStyle(e),height=e.getBoundingClientRect().height;
    const plain=e.cloneNode();plain.textContent=e.textContent;e.after(plain);const plainHeight=plain.getBoundingClientRect().height;plain.remove();
    return {ratio:parseFloat(style.fontSize)/parseFloat(base.fontSize),before:style.marginInlineStart,after:style.marginInlineEnd,height,plainHeight};
   });
   assert(Math.abs(metrics.ratio-1.2)<.001);assert.equal(metrics.before,'3px');assert.equal(metrics.after,'2px');
   assert.equal(metrics.height,metrics.plainHeight,'larger emoji do not increase the line height');
   assert(await page.locator('#chat-log').evaluate(e=>e.scrollWidth<=e.clientWidth),'message text still fits the chat');
   await mkdir(new URL('../.local/chat-emoji/',import.meta.url),{recursive:true});
   await page.screenshot({path:new URL(`../.local/chat-emoji/${theme}-${width}-messages.png`,import.meta.url).pathname});

   const input=page.locator('#chat-text'),button=page.locator('#chat-emoji-toggle'),panel=page.locator('#chat-emoji-panel');
   await input.fill('Hello WORLD!');
   await input.evaluate(e=>{e.focus();e.setSelectionRange(6,11);e.dispatchEvent(new Event('select'));});
   if(width===1280){
    failData=true;await button.click();
    await panel.getByText('Could not load emojis. Close and try again.').waitFor();
    assert.equal(await input.inputValue(),'Hello WORLD!');
    await panel.getByRole('button',{name:'Close emoji picker'}).click();failData=false;
   }
   await button.click();
   const search=panel.locator('input[type=search]');
   await search.waitFor();
   await search.fill('corn');
   const unicorn=panel.getByRole('option',{name:/unicorn/i});
   await unicorn.waitFor();
   assert.equal(posts.length,0);
   const box=await panel.boundingBox();
   assert(box.x>=0&&box.x+box.width<=width,'picker fits phone width');
   assert(box.y>=0&&box.y+box.height<=700+(width===1280?200:0),'picker fits viewport');
   assert(await panel.evaluate(e=>e.scrollWidth<=e.clientWidth),'no horizontal picker overflow');
   await mkdir(new URL('../.local/chat-emoji/',import.meta.url),{recursive:true});
   await page.screenshot({path:new URL(`../.local/chat-emoji/${theme}-${width}.png`,import.meta.url).pathname});
   await unicorn.click();
   await page.waitForFunction(()=>document.querySelector('#chat-text').value==='Hello 🦄!');
   assert.equal(await input.inputValue(),'Hello 🦄!','insert replaces selection');
   assert(await panel.isHidden());assert.equal(posts.length,0,'selecting never sends');
   await button.click();await search.fill('zzzznonexistent');await page.waitForFunction(()=>document.querySelector('emoji-picker').shadowRoot.querySelectorAll('[role=option][id^=emo-]').length===0);
   await search.press('Escape');assert(await panel.isHidden());assert(await page.locator('#chat-panel').isVisible());assert(await button.evaluate(e=>e===document.activeElement));
   await button.click();await search.fill('rocket');await panel.getByRole('option',{name:/^🚀,/}).click();
   await page.waitForFunction(()=>document.querySelector('#chat-text').value==='Hello 🦄🚀!');
   assert.equal(await input.inputValue(),'Hello 🦄🚀!');
   await input.fill('x'.repeat(279));await button.click();await search.fill('rocket');await panel.getByRole('option',{name:/^🚀,/}).click();
   assert.equal((await input.inputValue()).length,279,'never truncates an emoji at the limit');
   await panel.getByText(/No room/).waitFor();
   await panel.getByRole('button',{name:'Close emoji picker'}).click();
   await input.fill('draft 🦄');await page.locator('#chat-send').click();await page.locator('#chat-error').getByText('Try again.').waitFor();
   assert.equal(await input.inputValue(),'draft 🦄','failed send retains emoji draft');
   await button.click();await input.click();assert(await panel.isHidden(),'outside picker dismisses only picker');assert(await page.locator('#chat-panel').isVisible());
   await button.click();await search.fill('');
   await panel.getByRole('tab',{name:'Animals and nature'}).click();
   await panel.getByRole('button',{name:/Choose a skin tone/}).click();
   await panel.getByRole('option',{name:'Medium',exact:true}).click();
   await search.fill('rocket');await panel.getByRole('option',{name:/^🚀,/}).waitFor();
   await search.press('ArrowDown');await search.press('Enter');
   await page.waitForFunction(()=>document.querySelector('#chat-emoji-panel').hidden);
   assert((await input.inputValue()).includes('🚀'),'keyboard chooses search result');
   // Off/unauthorized is indistinguishable: no empty House calls tab or settings metadata.
   await button.click();await search.waitFor();
   assert(await panel.getByRole('tab',{name:'House calls',includeHidden:true}).isHidden());
   await panel.getByRole('button',{name:'Close emoji picker'}).click();
   houseCalls=[{id:'confetti-cannon',label:'Family time'},{id:'tea-break',label:'Break time'}];
   await button.click();const callTab=panel.getByRole('tab',{name:'House calls',exact:true});await callTab.waitFor();
   await callTab.click();assert.equal(await panel.locator('.house-call-card').count(),2);
   await panel.locator('.house-call-card img').first().evaluate(el=>el.decode());
   assert(await panel.evaluate(el=>el.scrollWidth<=el.clientWidth),'third tab fits without horizontal overflow');
   const callBounds=await callTab.boundingBox(),panelBounds=await panel.boundingBox();
   assert(callBounds.x>=panelBounds.x&&callBounds.x+callBounds.width<=panelBounds.x+panelBounds.width);
   await page.screenshot({path:new URL(`../.local/chat-emoji/${theme}-${width}-house-calls.png`,import.meta.url).pathname});
   const draft=await input.inputValue(),before=posts.length;
   await panel.getByRole('button',{name:'Send Family time',exact:true}).click();
   await page.locator('.chat-message .house-call-card').waitFor();
   await page.waitForFunction(()=>!document.querySelector('#library-chat').dataset.sending);
   assert.equal(posts.length,before+1,'picking a house call sends immediately');
   assert.deepEqual(Object.keys(posts.at(-1)).sort(),['client_id','house_call'],'client sends only the image ID and retry ID');
   assert.equal(await input.inputValue(),draft,'house calls preserve the text draft');
   assert(await panel.isHidden());assert.equal(await page.evaluate(()=>houseCallBursts),1,'sender celebrates a queued call once');
   const card=page.locator('.chat-message .house-call-card');
   assert.equal(await card.getAttribute('class'),'screenshot-tile house-call-card');
   assert.equal(await card.locator('span').textContent(),'Family time');
   await card.locator('img').evaluate(el=>el.decode());
   assert(await page.locator('#chat-log').evaluate(el=>el.scrollWidth<=el.clientWidth),'house call card fits existing chat layout');
   await page.evaluate(()=>testChatPoll());assert.equal(await page.evaluate(()=>houseCallBursts),1,'unchanged poll does not replay');
   triggerCall=false;await button.click();await callTab.click();await panel.getByRole('button',{name:'Send Break time'}).click();
   await page.waitForFunction(()=>document.querySelectorAll('.chat-message .house-call-card').length===2);
   await page.waitForFunction(()=>!document.querySelector('#library-chat').dataset.sending);
   assert.equal(await page.evaluate(()=>houseCallBursts),1,'cooldown/quiet-hours card has no confetti');
   messages.push({id:messages.at(-1).id+1,author:{id:'friend',name:'Friend'},text:'',created:Date.now()/1000,house_call:houseCalls[0],house_call_triggered:true});
   await page.evaluate(()=>testChatPoll());assert.equal(await page.evaluate(()=>houseCallBursts),2,'other live viewers celebrate');
   assert.equal(await page.locator('#chat-announcement').textContent(),'Friend: '+houseCalls[0].label,'announce the house-call label instead of empty text');
   await page.locator('#chat-log').evaluate(el=>el.scrollTo(0,0));await page.evaluate(()=>testChatPoll());
   assert.equal(await page.evaluate(()=>houseCallBursts),2,'scrollback cannot replay confetti');
   await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,value:true});document.dispatchEvent(new Event('visibilitychange'));});
   messages.push({...messages.at(-1),id:messages.at(-1).id+1});
   await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,value:false});document.dispatchEvent(new Event('visibilitychange'));});
   await page.waitForFunction(()=>document.querySelectorAll('.chat-message').length===9);
   assert.equal(await page.evaluate(()=>houseCallBursts),2,'returning to a hidden tab does not replay missed calls');
   await page.reload();await page.locator('#chat-toggle').click();await page.locator('.chat-message .house-call-card').first().waitFor();
   assert.equal(await page.evaluate(()=>houseCallBursts),0,'reload is history, not a live event');
   await button.click();await callTab.waitFor();await panel.getByRole('button',{name:'Close emoji picker'}).click();
   houseCalls=[];await button.click();await search.waitFor();assert(await panel.getByRole('tab',{name:'House calls',includeHidden:true}).isHidden());
   await panel.getByRole('button',{name:'Close emoji picker'}).click();
   if(width<1000){
    // Synthetic visual viewport models the keyboard; desktop emulation cannot open a real OS keyboard.
    await button.click();
    await page.evaluate(()=>{
     const viewport=new EventTarget();
     Object.assign(viewport,{width:innerWidth,height:360,offsetTop:0,offsetLeft:0});
     Object.defineProperty(window,'visualViewport',{configurable:true,value:viewport});
     window.dispatchEvent(new Event('resize'));
    });
    await page.waitForFunction(()=>document.querySelector('#library-chat').hasAttribute('data-short'));
    await search.fill('rocket');const result=panel.getByRole('option',{name:/^🚀,/});
    await result.waitFor();await result.scrollIntoViewIfNeeded();
    const bounds=await result.boundingBox();assert(bounds.y>=0&&bounds.y+bounds.height<=360,'search result usable above keyboard');
    await result.click();await page.waitForFunction(()=>document.querySelector('#chat-emoji-panel').hidden);
   }
   assert.deepEqual(await page.evaluate(()=>violations),[],'strict CSP satisfied');
   assert.deepEqual(errors,[]);
   await context.close();
  }
 }
 console.log('PASS: larger spaced emoji, intact graphemes, unchanged line height and copied text, safe message rendering; substring search, selection/cursor insertion, limits, no accidental sends, retry draft, Escape/outside dismissal, local assets and strict CSP; house-call availability, immediate ID-only sending, screenshot cards, draft preservation and live-only confetti; both themes at 1280/390/320px.');
}finally{await browser.close();}
