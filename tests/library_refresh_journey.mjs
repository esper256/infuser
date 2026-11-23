// Real-browser performance/refresh regression with a 40-game, 40-friend library.
// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs INFUSER_CHROMIUM=/path/to/chromium node tests/library_refresh_journey.mjs
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const root=new URL('../',import.meta.url);
const browser=await chromium.launch({headless:true,executablePath:process.env.INFUSER_CHROMIUM});
const me={player_id:'self',name:'Alex',admin:true,avatars:[]};
const tags=['Arcade','Chill','Cooperative','Adventure','Puzzle','Racing','Strategy','Long category label'].map((label,i)=>({id:'tag-'+i,label,count:40}));
const friends=Array.from({length:40},(_,i)=>({id:'friend-'+i,name:'Friend '+i}));
let apps=Array.from({length:40},(_,i)=>({id:'game-'+i,title:'Game '+i,description:'Original description',author:friends[i],card_art_url:'/art',tags:tags.map(t=>t.id),can_edit_tags:true,players:[],join_targets:[],reactions:[],updated:1}));
let messages=[],heldStars=new Map(),heldLibrary=[],heldHealth=[],holdLibrary=false,holdHealth=false,libraryRequests=0,healthRequests=0;
try{
 const context=await browser.newContext({viewport:{width:1200,height:850},serviceWorkers:'block'});
 await context.route('https://library.test/**',async route=>{
  const path=new URL(route.request().url()).pathname;
  if(path==='/api/me')return route.fulfill({json:me});
  if(path==='/api/library'){
   libraryRequests++;if(holdLibrary){heldLibrary.push(route);return;}
   return route.fulfill({json:{apps,friends,tags}});
  }
  if(/^\/api\/apps\/[^/]+\/star$/.test(path)){
   heldStars.set(path.split('/')[3],route);return;
  }
  if(path==='/api/admin/peer-health'){
   healthRequests++;if(holdHealth){heldHealth.push(route);return;}
   return route.fulfill({json:{sessions:[]}});
  }
  if(path==='/api/admin/game-authors')return route.fulfill({json:{games:[],members:[]}});
  if(path==='/api/admin/members')return route.fulfill({json:{members:Object.fromEntries(friends.map(p=>[p.id,{name:p.name,enabled:true}]))}});
  if(path==='/api/admin/credentials')return route.fulfill({json:{credentials:friends.map(p=>({id:p.id,label:p.name,app:'game-0'}))}});
  if(path==='/api/chat')return route.fulfill({json:{messages}});
  if(path==='/api/feedback')return route.fulfill({json:{available:false}});
  if(path==='/api/client-state')return route.fulfill({json:{game_open:true,version:'test'}});
  const file=path==='/library'?'web/index.html':path==='/art'?'docs/design-explorations/card-art-guide/firefly-drift.webp':path.startsWith('/assets/')?'web/'+path.slice(8):null;
  if(file)try{
   let body=await readFile(new URL(file,root));
   if(file==='web/clubhouse.js'){
    if(process.env.INFUSER_CLUBHOUSE_SOURCE)body=await readFile(process.env.INFUSER_CLUBHOUSE_SOURCE);
    body=body.toString()+'\nwindow.refreshLibrary=refresh;window.refreshHealth=refreshNetworkHealth;window.libraryRefreshing=()=>refreshing;';
   }
   return route.fulfill({body,contentType:file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':file.endsWith('.webp')?'image/webp':'text/html'});
  }catch{}
  return route.fulfill({status:404,body:''});
 });
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(()=>{
  window.work={resize:0,mutations:0,loops:0,shifts:0};
  const NativeResizeObserver=ResizeObserver;
  window.ResizeObserver=class extends NativeResizeObserver{constructor(callback){super((...args)=>{work.resize++;callback(...args);});}};
  new MutationObserver(records=>work.mutations+=records.length).observe(document,{subtree:true,childList:true,attributes:true,characterData:true});
  new PerformanceObserver(list=>{for(const entry of list.getEntries())work.shifts+=entry.value;}).observe({type:'layout-shift'});
  addEventListener('error',event=>{if(event.message.includes('ResizeObserver'))work.loops++;});
 });
 await page.goto('https://library.test/library');await page.waitForFunction(()=>window.refreshHealth);
 assert.equal(await page.locator('.game').count(),40);
 const settle=()=>page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
 const reset=()=>page.evaluate(()=>{work={resize:0,mutations:0,loops:0,shifts:0};});
 const cdp=await context.newCDPSession(page);await cdp.send('Performance.enable');
 const metrics=async()=>Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map(m=>[m.name,m.value]));
 for(const theme of ['nightglass','cartridge-club'])for(const width of [1200,650,390,320]){
  await page.setViewportSize({width,height:850});await page.evaluate(theme=>InfuserThemes.select(theme),theme);
  await page.waitForTimeout(350);await reset();
  await page.waitForTimeout(180);
  assert.deepEqual(await page.evaluate(()=>work),{resize:0,mutations:0,loops:0,shifts:0},`${theme} ${width}: observers must settle at idle`);
  await page.locator('#chat-toggle').click();await page.locator('#chat-text').fill('Keep my unfinished message');
  await settle();await reset();const before=await metrics();
  for(let i=0;i<5;i++)await page.evaluate(()=>refreshLibrary());
  await settle();const after=await metrics();
  assert.deepEqual(await page.evaluate(()=>work),{resize:0,mutations:0,loops:0,shifts:0},`${theme} ${width}: unchanged data must leave the DOM alone`);
  assert.equal(after.LayoutCount-before.LayoutCount,0,'unchanged refresh must not force layouts');
  assert.equal(await page.locator('#chat-text').inputValue(),'Keep my unfinished message');
  assert(await page.locator('#chat-text').evaluate(e=>e===document.activeElement));
  console.log(`${theme} ${width}: five unchanged refreshes, 0 mutations/layouts, ${Math.round((after.TaskDuration-before.TaskDuration)*1000)} ms browser task time`);
  // An empty room also keeps its message and speech-tail nodes on repeated polls.
  await reset();
  const chat=page.waitForResponse('**/api/chat');await page.evaluate(()=>window.dispatchEvent(new Event('online')));await (await chat).finished();await settle();
  assert.equal((await page.evaluate(()=>work)).mutations,0,'unchanged chat poll must not rewrite the panel');
  await page.locator('#chat-log').press('Escape');
 }
 await page.setViewportSize({width:1200,height:850});await settle();
 const first=page.locator('[data-app="game-0"]'),other=page.locator('[data-app="game-1"]');
 await first.locator('.game-open').focus();
 await page.evaluate(()=>{
  window.retained={title:document.activeElement,art:document.querySelector('.game-art img'),tags:document.querySelector('.game-tags'),other:document.querySelector('[data-app="game-1"]').innerHTML};
 });
 apps[0].players=[{name:'New player'}];apps[0].join_targets=[{name:'New player',session_id:'new-session',player_id:'new-player'}];
 apps[0].reactions=[{id:'reactor',name:'Reactor',emoji:'⭐',updated:Date.now()/1000,text:'Great!'}];
 await page.evaluate(()=>refreshLibrary());
 assert.equal(await first.locator('.game-live-menu .game-player-name').textContent(),'New player');
 assert.equal(await first.locator('.play-menu .players').count(),0,'players are listed only in the portrait menu');
 assert.equal(await first.locator('.play-menu').getByRole('button',{name:'Join New player',exact:true,includeHidden:true}).count(),1,'Join actions remain available');
 assert.equal(await first.locator('.game-buzz span').count(),1);
 assert(await page.evaluate(()=>retained.title===document.activeElement&&retained.art===document.querySelector('.game-art img')&&retained.tags===document.querySelector('.game-tags')),'presence/reactions retain title focus, cover and tags');
 assert.equal(await other.innerHTML(),await page.evaluate(()=>retained.other),'unrelated cards remain unchanged');
 // Retained handlers must open the newest data, not the old API response.
 await first.locator('.creator').click();assert(await page.locator('#reaction-dialog').isVisible());await page.locator('#close-reaction').click();
 await first.locator('.game-open').click();assert.equal(await page.locator('.game-feedback-row').count(),1);assert.equal(await page.locator('#game-detail .play-menu .players').count(),0);await page.locator('#close-game-detail').click();
 apps[0].title='Renamed game';tags[0].label='Renamed tag';await page.evaluate(()=>refreshLibrary());
 assert.equal(await first.locator('.game-open').textContent(),'Renamed game');
 assert.equal(await first.locator('.tag-chip').first().textContent(),'Renamed tag');
 // New activity can arrive without replacing the row currently being read.
 await page.locator('#recent-toggle').click();await page.locator('#feed .feed-row').first().focus();
 await page.evaluate(()=>{window.focusedEvent=document.activeElement;});
 apps[1].updated=Date.now()/1000+2;await page.evaluate(()=>refreshLibrary());
 assert(await page.evaluate(()=>focusedEvent===document.activeElement&&focusedEvent.isConnected),'new activity preserves the focused row');
 assert.equal(await page.locator('#feed time').count(),1);assert.equal(await page.locator('#feed .feed-row').first().locator('time').count(),1);
 await page.locator('#close-recent').click();
 // Open controls are not dismantled under a pointer while backend state changes.
 await first.locator('.play-menu summary').click();apps[0].players=[];
 await page.evaluate(()=>refreshLibrary());assert(await first.locator('.play-menu').evaluate(e=>e.open));
 await first.locator('.play-menu summary').click();await page.evaluate(()=>refreshLibrary());assert.equal(await first.locator('.game-live').count(),0);
 // Star requests for different dialogs are independent, and old Library snapshots
 // cannot override a pending or newly acknowledged star.
 const star=page.locator('#game-detail .detail-star-btn');
 const openGame=async card=>{await card.locator('.game-open').click();await star.waitFor();};
 const closeGame=()=>page.locator('#close-game-detail').click();
 const waitForHeld=async get=>{
  for(let i=0;i<100;i++){
   const route=get();if(route)return route;
   await new Promise(resolve=>setTimeout(resolve,10));
  }
  assert.fail('Fixture request was not intercepted');
 };
 const toggle=async id=>{
  const request=page.waitForRequest(req=>req.url().endsWith('/api/apps/'+id+'/star')&&req.method()==='PUT');
  await star.click();await request;await waitForHeld(()=>heldStars.get(id));
 };
 const holdRefresh=async()=>{
  const request=page.waitForRequest('**/api/library');
  await page.evaluate(()=>{window.starRefresh=refreshLibrary();});await request;
  await waitForHeld(()=>heldLibrary[0]);
 };
 const starState=(value,pending)=>page.waitForFunction(([value,pending])=>{
  const star=document.querySelector('#game-detail .detail-star-btn');
  return star?.dataset.starred===String(value)&&star.getAttribute('aria-disabled')===String(pending);
 },[value,pending]);
 await openGame(first);await toggle('game-0');await starState(true,true);
 assert(await star.evaluate(button=>button===document.activeElement),'pending star retains keyboard focus');
 await closeGame();await openGame(other);await toggle('game-1');await starState(true,true);
 assert.deepEqual([...heldStars.keys()],['game-0','game-1']);
 const staleApps=structuredClone(apps);
 holdLibrary=true;await holdRefresh();
 await heldLibrary.shift().fulfill({json:{apps:staleApps,friends,tags}});
 await page.evaluate(()=>starRefresh);
 await starState(true,true);
 await closeGame();await openGame(first);await starState(true,true);
 await holdRefresh();
 apps[0].starred=true;
 await heldStars.get('game-0').fulfill({json:{ok:true,starred:true}});
 await starState(true,false);
 const followup=page.waitForRequest('**/api/library');
 await heldLibrary.shift().fulfill({json:{apps:staleApps,friends,tags}});
 await followup;await waitForHeld(()=>heldLibrary[0]);
 await starState(true,false); // Before the post-write request is allowed to return.
 await closeGame();await openGame(other);await starState(true,true);
 holdLibrary=false;await heldLibrary.shift().fulfill({json:{apps,friends,tags}});
 await page.evaluate(()=>starRefresh);
 await heldStars.get('game-1').fulfill({status:503,json:{error:'Could not star Game 1.'}});
 await starState(false,false);await closeGame();await openGame(first);await starState(true,false);await closeGame();
 await page.waitForFunction(()=>!libraryRefreshing());
 // Slow requests cannot accumulate; timeout/failure keeps the mounted library.
 holdLibrary=true;const beforeRequests=libraryRequests;
 const waiting=page.waitForRequest('**/api/library');
 await page.evaluate(()=>{window.currentCard=document.querySelector('.game');window.pendingRefresh=refreshLibrary();});await waiting;

 await page.evaluate(()=>Promise.all(Array.from({length:20},()=>refreshLibrary())));
 assert.equal(libraryRequests-beforeRequests,1,'only one library request may be in flight');
 await page.evaluate(()=>pendingRefresh); // Real 12-second timeout, no fake clock.
 assert(await page.evaluate(()=>currentCard===document.querySelector('.game')),'timeout must preserve the library');
 assert.match(await page.locator('#notice').textContent(),/timed out|timeout/i);
 for(const route of heldLibrary)await route.abort().catch(()=>{});heldLibrary=[];
 holdLibrary=false;await page.evaluate(()=>refreshLibrary());assert.equal(libraryRequests-beforeRequests,2,'polling recovers after timeout');
 holdHealth=true;const beforeHealth=healthRequests,health=page.waitForRequest('**/api/admin/peer-health');
 await page.locator('#account-trigger').click();await page.locator('#open-admin').click();
 await page.locator('#members > div').last().waitFor({state:'attached'});
 assert.equal(await page.locator('#admin .admin-region').count(),6);
 assert.equal(await page.locator('#admin .admin-region[open]').count(),0,'admin regions start folded');
 assert.equal(await page.locator('#members').isVisible(),false);assert.equal(await page.locator('#credentials').isVisible(),false);
 await page.evaluate(()=>refreshHealth());assert.equal(healthRequests,beforeHealth,'folded diagnostics do not poll');
 const membersSummary=page.locator('#admin summary').filter({hasText:/^Members$/});
 await membersSummary.focus();await page.keyboard.press('Enter');assert(await page.locator('#members').isVisible());
 const accessSummary=page.locator('#admin summary').filter({hasText:/^Agent access$/});await accessSummary.click();assert(await page.locator('#credentials').isVisible());
 await page.evaluate(()=>refreshLibrary());assert(await page.locator('#members').isVisible(),'refresh preserves expanded regions');
 await membersSummary.click();await accessSummary.click();
 await page.locator('#admin summary').filter({hasText:'Network diagnostics'}).click();await health;
 await page.evaluate(()=>Promise.all(Array.from({length:20},()=>refreshHealth())));
 assert.equal(healthRequests-beforeHealth,1,'slow diagnostics must not accumulate requests');
 for(const route of heldHealth)await route.fulfill({json:{sessions:[]}});heldHealth=[];
 await page.waitForFunction(()=>document.querySelector('#network-health')?.textContent.includes('No active peer sessions'));
 await page.evaluate(async()=>{Object.defineProperty(document,'hidden',{value:true,configurable:true});await refreshHealth();delete document.hidden;});
 assert.equal(healthRequests-beforeHealth,1,'background diagnostics are paused');holdHealth=false;
 await page.locator('#admin summary').filter({hasText:'Network diagnostics'}).click();await page.evaluate(()=>refreshHealth());
 assert.equal(healthRequests-beforeHealth,1,'folding diagnostics stops polling');
 await page.locator('#close-admin').click();
 await page.evaluate(()=>refreshHealth());assert.equal(healthRequests-beforeHealth,1,'closed admin dialog does not poll diagnostics');
 // The empty-library state is also stable across repeated backend snapshots.
 apps=[];await page.evaluate(()=>refreshLibrary());await settle();await reset();await page.evaluate(()=>refreshLibrary());await settle();
 assert.equal((await page.evaluate(()=>work)).mutations,0,'empty-library refresh is stable');
 assert.deepEqual(errors,[]);
 console.log('PASS: idle observers settle; no-op refresh/chat causes no DOM churn or layouts; live presence, reactions, title/tag edits, focus, drafts and menus survive; slow polling is bounded and recovers.');
}finally{await browser.close();}
