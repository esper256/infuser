// Browser regression for update safety and the persistent Hidden disclosure.
// Set INFUSER_PLAYWRIGHT and INFUSER_CHROMIUM for an existing browser installation.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.INFUSER_CHROMIUM});
try {
 const page=await browser.newPage();
 await page.route('https://updates.test/**',async route=>route.fulfill({
  contentType:route.request().url().endsWith('.js')?'text/javascript':'text/html',
  body:route.request().url().endsWith('.js')?await readFile(new URL('../web/client-updates.js',import.meta.url),'utf8'):'<meta name="infuser-client-version" content="old"><main></main>'
 }));
 await page.goto('https://updates.test/');
 const results=await page.evaluate(async()=>{
  const {initClientUpdates}=await import('/client-updates.js'),results=[];
  for(const [name,markup,gameOpen,expected] of [
   ['Hidden shelf open','<details id="hidden-games" open><summary>Hidden</summary></details>',false,1],
   ['Play options open','<details class="play-menu" open><summary>Options</summary></details>',false,0],
   ['Profile menu open','<details class="profile-menu" open><summary>Profile</summary></details>',false,0],
   ['Admin tools open','<dialog id="admin" open></dialog>',false,0],
   ['Game details open','<dialog open></dialog>',false,0],
   ['Chat draft','<textarea id="chat-text">Unfinished</textarea>',false,0],
   ['Screenshot draft','<div class="screenshot-draft"></div>',false,0],
   ['Cleared screenshot draft','<div class="screenshot-draft" hidden></div>',false,1],
   ['Launching','<div data-launching></div>',false,0],
   ['Sending chat','<div id="library-chat" data-sending></div>',false,0],
   ['Game open','',true,0],
   ['Safe to update','',false,1]
  ]){
   document.querySelector('main').innerHTML=markup;
   let time=0,reloads=0,tick;
   initClientUpdates({api:async()=>({version:'new',game_open:gameOpen}),now:()=>time,
    window:{setInterval:fn=>{tick=fn;},addEventListener(){},location:{reload(){reloads++;}}}});
   await Promise.resolve();time=10000;await tick();await tick();
   results.push({name,reloads,expected});
  }
  return results;
 });
 for(const {name,reloads,expected} of results)assert.equal(reloads,expected,name);
 console.log('PASS: expanded Hidden shelf permits one safe refresh; games, menus, dialogs, launches, chat drafts and sends defer it.');
} finally {await browser.close();}
