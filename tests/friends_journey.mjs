// Avatar Join Friend journey. Mock API, real Library UI and browser popups.
// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs node tests/friends_journey.mjs
// Set INFUSER_TOUCH=1 to also exercise touch opening and coarse-pointer styles.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const root=new URL('../',import.meta.url);
const browser=await chromium.launch({headless:true,...(process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
const context=await browser.newContext({hasTouch:process.env.INFUSER_TOUCH==='1'});const page=await context.newPage();
const errors=[];page.on('pageerror',e=>errors.push(e.message));
const me={player_id:'self',name:'Alex',admin:false,avatar:'circuit-fox',avatars:[{id:'circuit-fox',name:'Circuit Fox'},{id:'moss-knight',name:'Moss Knight'}]};
let friends=[{id:'a',name:'Rowan',avatar:'moss-knight'},{id:'b',name:'Sam',avatar:'circuit-fox'},{id:'c',name:'Quinn',avatar:'moss-knight'}];
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
 if(path==='/api/chat')return route.fulfill({json:{messages:[]}});
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
try {
 await page.goto('https://library.test/library');await page.waitForFunction(()=>window.refreshLibrary);
 const first=page.locator('.friend-menu[data-key="a"]');
 await first.locator('summary').focus();await page.keyboard.press('Enter');
 const join=first.getByRole('button',{name:'Join Firefly Drift'});await join.focus();
 await page.evaluate(()=>refreshLibrary());
 assert.equal(await first.getAttribute('open'),'');assert.equal(await join.evaluate(el=>el===document.activeElement),true);
 let popup=page.waitForEvent('popup');await join.click();let gamePage=await popup;await gamePage.waitForURL('**/playing');await gamePage.close();
 assert.deepEqual(launches.pop(),{guest:false,intent:{kind:'join_friend',session_id:'session-a',player_id:'a'}});
 await page.setViewportSize({width:320,height:760});
 const more=page.locator('.friend-menu[data-key="overflow"]');await more.locator('summary').click();
 const panel=more.locator('.friend-actions');const box=await panel.boundingBox();assert.ok(box.x>=15&&box.x+box.width<=305,JSON.stringify(box));
 assert.match(await panel.innerText(),/Sam/);assert.match(await panel.innerText(),/Quinn/);
 popup=page.waitForEvent('popup');await more.getByRole('button',{name:'Join Firefly Drift'}).click();gamePage=await popup;await gamePage.waitForURL('**/playing');await gamePage.close();
 assert.equal(launches.pop().intent.player_id,'b');
 // Presence changes update open menus and return focus safely when a session ends.
 await first.locator('summary').click();await first.getByRole('button').focus();friends[0].join_targets=[];
 await page.evaluate(()=>refreshLibrary());assert.equal(await first.getByRole('button').count(),0);assert.equal(await first.locator('summary').evaluate(el=>el===document.activeElement),true);
 await page.keyboard.press('Escape');assert.equal(await first.getAttribute('open'),null);
 // Session can end between the poll and click: report it, never launch a new room.
 stale=true;await more.locator('summary').click();popup=page.waitForEvent('popup');await more.getByRole('button').click();gamePage=await popup;
 await page.waitForFunction(()=>document.body.innerText.includes('That session ended. Try again.'));
 assert.equal(launches.length,1);assert.equal(launches[0].intent.kind,'join_friend');
 await page.waitForFunction(()=>!document.querySelector('.presence-person[data-launching]'));
 assert.equal(await more.locator('button').textContent(),'Join Firefly Drift');
 // Inspection never offers actions that would make the operator appear in that group.
 me.inspecting=true;await page.reload();await page.waitForFunction(()=>window.refreshLibrary);await page.locator('.friend-menu').first().locator('summary').click();
 assert.equal(await page.locator('.friend-actions button').count(),0);
 assert.deepEqual(errors,[]);console.log('PASS: avatar/overflow joins, exact session intent, polling/focus, stale sessions, mobile bounds, inspection.');
} finally { await browser.close(); }
