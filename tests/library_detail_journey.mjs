// Shared Library presentation/interaction regression with an isolated mock API.
// Set INFUSER_PLAYWRIGHT and INFUSER_CHROMIUM when using an existing browser install.
import {readFile,mkdir} from 'node:fs/promises';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const root=new URL('../',import.meta.url),out=new URL('.local/library-detail-review/',root);
await mkdir(out,{recursive:true});
const avatars=[...String(await readFile(new URL('src/portal/avatars.rs',root))).matchAll(/avatar!\("([a-z-]+)", "([^"]+)"\)/g)].map(([,id,name])=>({id,name}));
const me={name:'KwamiKwami',player_id:'one',admin:true,avatar:'loot-raccoon',avatars};
const author={id:'one',name:me.name,avatar:me.avatar},friend={id:'two',name:'OrbitFox',avatar:'ember-raven'};
const now=Math.floor(Date.now()/1000),tags=['3d','action','adventure','arcade','chill','co-op','exploration','sandbox','shooter','simulation','survival'];
const game={id:'firefly-drift',title:'Firefly Drift',description:'A quiet flight through a glowing forest.',author,updated:now-60,card_art_url:'/test-art/firefly',url:'https://library.test/games/firefly-drift',tags:['3d','arcade','chill'],can_edit_tags:true,players:[],join_targets:[{name:'OrbitFox',session_id:'session',player_id:'two'}],reactions:[{...friend,emoji:'👏',text:'Love the atmosphere.',updated:now,mine:false},{...author,emoji:'⭐',text:'',updated:now-10,mine:true}]};
const apps=[game,{...game,id:'nocomgame',title:'NoCOMgame',description:null,author:friend,card_art_url:null,tags:['3d','sandbox','action','survival','exploration'],reactions:[]},{...game,id:'event-horizon',title:'Event Horizon',author:friend,card_art_url:'/test-art/event',reactions:[]},{...game,id:'dodge',title:'Dodge the Creeps',author:null,preview:true,card_art_url:null,reactions:[]},{...game,id:'lantern',title:'Lantern Lake',author:null,preview:true,card_art_url:null,reactions:[]}];
const recentReactions=n=>Array.from({length:n},(_,i)=>({id:'buzz-'+i,name:'Player '+i,emoji:['⭐','👍','🚀','❤️','👏','💡'][i%6],updated:now-i-2,text:'',mine:false}));
apps[1].card_art_pending=true;apps[1].reactions=recentReactions(8);apps[2].reactions=recentReactions(12);apps[4].reactions=recentReactions(18);
let launches=[],writes=[],reads=0,unauthorized=false;
const browser=await chromium.launch({headless:true,executablePath:process.env.INFUSER_CHROMIUM});
try {
 const context=await browser.newContext({hasTouch:process.env.INFUSER_TOUCH==='1',viewport:{width:1500,height:1000},serviceWorkers:'block',permissions:['clipboard-read','clipboard-write']});
 await context.route('https://library.test/**',async route=>{
  const request=route.request(),path=new URL(request.url()).pathname,method=request.method();let data;
  if(method!=='GET')writes.push({path,body:request.postDataJSON()});
  if(path==='/api/me')return route.fulfill({status:unauthorized?401:200,json:unauthorized?{error:'Sign in'}:me});
  if(path==='/api/library'){reads++;data={apps,friends:[author,friend],tags:tags.map(id=>({id,label:id==='3d'?'3D':id[0].toUpperCase()+id.slice(1),count:apps.filter(g=>g.tags.includes(id)).length}))};}
  else if(path==='/api/chat')data={messages:[]};
  else if(path==='/api/client-state')data={version:'review',game_open:false};
  else if(path==='/api/admin/members')data={members:{}};
  else if(path==='/api/admin/credentials')data={credentials:[]};
  else if(path==='/api/admin/peer-health')data={sessions:[]};
  else if(path==='/api/profile'){me.avatar=request.postDataJSON().avatar;author.avatar=me.avatar;data={avatar:me.avatar};}
  else if(path.endsWith('/launch')){launches.push(request.postDataJSON());data={state:'ready',url:'https://library.test/game-fixture'};}
  else if(path.endsWith('/visibility')){const app=apps.find(g=>g.id===path.split('/')[4]);app.preview=!request.postDataJSON().listed;data={listed:!app.preview};}
  else if(path.endsWith('/reaction')){game.reactions=game.reactions.filter(r=>!r.mine);if(method==='PUT')game.reactions.push({...author,...request.postDataJSON(),updated:now+1,mine:true});data={ok:true};}
  else if(path.endsWith('/tags')){game.tags=request.postDataJSON().tags;data={ok:true};}
  else if(path==='/api/my-games')data={api_url:'https://library.test',app_id:'new-game',setup_code:'TEST-ONLY',expires_at:now+10800};
  else if(path==='/api/feedback')data={available:false};
  else if(path==='/game-fixture')return route.fulfill({contentType:'text/html',body:'<title>Test game</title>'});
  else {
   let file,type;
   if(path==='/test-art/firefly'){file='docs/design-explorations/card-art-guide/firefly-drift.webp';type='image/webp';}
   else if(path==='/test-art/event'){file='assets/avatars/event-horizon.png';type='image/png';}
   else if(path.startsWith('/assets/avatars/')){const [,id,size]=path.match(/\/avatars\/([^/]+)\/(\d+)\.webp$/);file=`assets/avatars/${id}-${size}.webp`;type='image/webp';}
   else if(path==='/assets/card-generating.webp'){file='assets/card-fallback/generating.webp';type='image/webp';}
   else if(path==='/assets/google-signin.png'){file='assets/google/sign-in-with-google-pill.png';type='image/png';}
   else if(path.startsWith('/assets/')){file='web/'+path.slice(8);type=path.endsWith('.js')?'text/javascript':'text/css';}
   else if(path==='/sw.js')return route.fulfill({status:404,body:''});
   else {file='web/index.html';type='text/html';}
   let body=await readFile(new URL(file,root));
   if(file==='web/clubhouse.js')body=body.toString()+'\nwindow.refreshTestLibrary=refresh;';
   return route.fulfill({body,contentType:type});
  }
  return route.fulfill({json:data});
 });
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto('https://library.test/library');await page.locator('#visible-games .game').first().waitFor();
 const card=()=>page.locator('#visible-games [data-app="firefly-drift"]'),dialog=page.locator('#game-detail');
 async function close(){await page.locator('#close-game-detail').click();await page.waitForFunction(()=>!document.querySelector('#game-detail').open);}
 async function open(){await card().locator('.game-open').click();await dialog.waitFor();}
 async function popup(action){const opening=page.waitForEvent('popup');await action();const tab=await opening;await tab.waitForURL('**/game-fixture');await tab.close();}
 // Pending placeholder transitions and author search use the real shared renderer.
 const pendingCard=()=>page.locator('#visible-games [data-app="nocomgame"]');
 assert.equal(await pendingCard().locator('.game-art img').getAttribute('src'),'/assets/card-generating.webp');
 await pendingCard().locator('.game-open').click();assert.equal(await dialog.locator('.detail-art img').getAttribute('src'),'/assets/card-generating.webp');await close();
 apps[1].card_art_pending=false;await page.evaluate(()=>refreshTestLibrary());assert.equal(await pendingCard().locator('.game-art img').count(),0);
 apps[1].card_art_pending=true;apps[1].card_art_url='/test-art/firefly';await page.evaluate(()=>refreshTestLibrary());assert.equal(await pendingCard().locator('.game-art img').getAttribute('src'),'/test-art/firefly');
 apps[1].card_art_url=null;await page.evaluate(()=>refreshTestLibrary());
 await page.locator('#game-search').fill('kWaMi');assert.equal(await page.locator('#visible-games .game').count(),1);
 await page.locator('#game-search').fill(' ORBITFOX ');assert.equal(await page.locator('#visible-games .game').count(),2);
 await page.locator('#game-search').fill('');
 // Only the last 28 days contribute; a busy card never grows past six faces.
 for(const [n,faces] of [[0,0],[1,1],[8,4],[12,5],[50,6]]){
  apps[1].reactions=[...recentReactions(n),{id:'old',name:'Old',emoji:'⭐',updated:now-29*86400,text:''}];await page.evaluate(()=>refreshTestLibrary());
  assert.equal(await pendingCard().locator('.game-buzz span').count(),faces);
 }
 apps[1].reactions=recentReactions(8);await page.evaluate(()=>refreshTestLibrary());
 assert.equal(new Set(await pendingCard().locator('.game-buzz span').allTextContents()).size,4);
 await pendingCard().locator('.game-buzz').click();assert(await dialog.isVisible());assert(await page.evaluate(()=>document.activeElement.classList.contains('game-feedback-row')));await close();
 for(const theme of ['nightglass','cartridge-club']){
  await page.evaluate(t=>InfuserThemes.select(t),theme);
  for(const width of [1500,900,390,320]){
   await page.setViewportSize({width,height:width>650?1000:844});
   assert.equal(await page.locator('#visible-games .game').count(),3);assert.equal(await page.locator('#hidden-game-list .game').count(),2);
   await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),`${theme} ${width} page overflow`);
   assert.equal(await page.locator('#friends [data-player-id="one"]').count(),1);
   await page.locator('#hidden-games').evaluate(e=>e.open=true);
   await page.evaluate(()=>document.activeElement.blur());
   assert(await page.locator('#visible-games .game').evaluateAll(cards=>cards.every(card=>{const buzz=card.querySelector('.game-buzz'),play=card.querySelector('.actions'),heading=card.querySelector('.game-heading');if(!buzz)return true;const b=buzz.getBoundingClientRect(),p=play.getBoundingClientRect(),h=heading.getBoundingClientRect();return b.right<=p.left+1&&b.top>=h.bottom;})),'reaction stacks must not overlap controls');
   await page.screenshot({path:new URL(`${theme}-${width}.png`,out).pathname,fullPage:true});
   await open();await page.waitForFunction(()=>document.querySelector('.detail-art img').complete);
   assert.equal(await dialog.locator('.game-feedback-row').count(),2);
   assert.equal(await dialog.locator('.reaction-choice').count(),6);
   assert(await dialog.locator('.reaction-face').first().evaluate(e=>parseFloat(getComputedStyle(e).fontSize)>=44));
   assert.equal(await dialog.locator('.feedback-author img').first().getAttribute('src'),'/assets/avatars/ember-raven/128.webp');
   assert(await dialog.locator('.game-updated').textContent());
   assert(await dialog.evaluate(e=>e.scrollWidth<=e.clientWidth),`${theme} ${width} dialog overflow`);
   await page.screenshot({path:new URL(`${theme}-${width}-detail.png`,out).pathname});
   await page.keyboard.press('Escape');await page.waitForFunction(()=>!document.querySelector('#game-detail').open);
   assert(await card().locator('.game-open').evaluate(e=>e===document.activeElement));
  }
 }
 await page.setViewportSize({width:1500,height:1000});
 // Artwork opens details, without launching. Title also supports keyboard activation.
 const before=launches.length;await card().click({position:{x:150,y:150}});await dialog.waitFor();assert.equal(launches.length,before);await close();
 await card().locator('.game-open').focus();await page.keyboard.press('Enter');await dialog.waitFor();
 await page.locator('#account-trigger').evaluate(()=>InfuserThemes.select('nightglass'));assert(await dialog.isVisible());
 await dialog.locator('.play-menu summary').click();await page.keyboard.press('Escape');assert(await dialog.isVisible());assert.equal(await dialog.locator('.play-menu').getAttribute('open'),null);
 // Existing launch paths share their implementation between card and detail.
 await popup(()=>dialog.locator('.actions > button').click());assert.equal(launches.at(-1).guest,false);await close();
 await popup(()=>card().locator('.actions > button').click());assert(!(await dialog.isVisible()));
 await open();await dialog.locator('.play-menu summary').click();await popup(()=>dialog.getByRole('button',{name:'Play in guest mode',exact:true}).click());assert.equal(launches.at(-1).guest,true);
 await dialog.locator('.play-menu summary').click();await popup(()=>dialog.getByRole('button',{name:'Join OrbitFox',exact:true}).click());assert.equal(launches.at(-1).intent.kind,'join_friend');
 await dialog.locator('.play-menu summary').click();await dialog.getByRole('button',{name:'Copy game link',exact:true}).click();assert.equal(await page.evaluate(()=>navigator.clipboard.readText()),game.url);
 // Nested reaction/tag dialogs return to details, then refresh real content.
 await dialog.locator('.creator').click();await page.getByRole('radio',{name:'Great job',exact:true}).check();await page.locator('#reaction-text').fill('Fresh feedback');await page.locator('#reaction-form button[type="submit"]').click();
 await page.waitForFunction(()=>document.querySelector('#game-feedback').textContent.includes('Fresh feedback'));assert(await dialog.isVisible());
 await dialog.locator('.detail-tags').getByRole('button',{name:'Edit tags',exact:true}).click();await page.locator('#save-tags').click();await page.waitForFunction(()=>!document.querySelector('#tags-dialog').open);assert(await dialog.isVisible());await close();
 // Recently links target the referenced reaction, not a launch.
 await page.locator('#recent-toggle').click();await page.locator('#feed button').filter({hasText:'Fresh feedback'}).click();await dialog.waitFor();assert(await page.evaluate(()=>document.activeElement.classList.contains('game-feedback-row')));await close();
 if(process.env.INFUSER_TOUCH==='1'){
  // A long press opens Tags; its release must not also open game details.
  await page.setViewportSize({width:390,height:844});
  await card().dispatchEvent('pointerdown',{pointerType:'touch',isPrimary:true,pointerId:1,clientX:80,clientY:180});
  await page.locator('#tags-dialog').waitFor();await card().dispatchEvent('pointerup',{pointerType:'touch',pointerId:1});
  await card().dispatchEvent('click');assert(!(await dialog.isVisible()));await page.locator('#tags-dialog').evaluate(e=>e.close());
 }
 // Obvious reaction controls: choose, add an optional kind note, or send emoji-only.
 await page.setViewportSize({width:390,height:844});await open();
 await dialog.getByRole('button',{name:'React: Exciting',exact:true}).click();
 assert(await page.getByRole('radio',{name:'Exciting',exact:true}).isChecked());
 assert(await page.locator('#reaction-comment').isVisible());assert(await page.locator('#reaction-text').evaluate(e=>parseFloat(getComputedStyle(e).fontSize)>=22));
 await page.locator('#reaction-text').fill('');await page.getByRole('button',{name:'Send reaction',exact:true}).click();
 await page.waitForFunction(()=>document.querySelector('.reaction-choice[aria-label="React: Exciting"]').getAttribute('aria-pressed')==='true');
 assert.equal(game.reactions.find(r=>r.mine).text,'');assert.equal(game.reactions.find(r=>r.mine).emoji,'🚀');
 await dialog.locator('.creator').click();await page.locator('#remove-reaction').click();await page.waitForFunction(()=>!document.querySelector('#reaction-dialog').open);await page.evaluate(()=>refreshTestLibrary());
 await dialog.locator('.creator').click();assert(await page.locator('#reaction-comment').isHidden());
 await page.getByRole('radio',{name:'Loved it',exact:true}).check();await page.locator('#reaction-text').fill('I liked the glowing trail!');
 assert.equal(await page.locator('.reaction-burst:popover-open').count(),1);
 await page.screenshot({path:new URL('reaction-composer-phone.png',out).pathname});
 await page.getByRole('button',{name:'Send reaction',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('#reaction-dialog').open);await close();
 // Unread Recently is large and animated in both themes, clears on opening,
 // and retains a static highlight with reduced motion.
 for(const theme of ['nightglass','cartridge-club']){
  await page.evaluate(t=>InfuserThemes.select(t),theme);game.updated=Math.max(now,game.updated)+1;await page.evaluate(()=>refreshTestLibrary());
  const recent=page.locator('#recent-toggle');assert(await recent.evaluate(e=>e.classList.contains('has-updates')));
  assert(await recent.evaluate(e=>getComputedStyle(e).animationName==='recent-excitement'));
  assert(await recent.evaluate(e=>getComputedStyle(e).backgroundColor!=='rgba(0, 0, 0, 0)'&&parseFloat(getComputedStyle(e).fontSize)>=15));
  await page.emulateMedia({reducedMotion:'reduce'});assert.equal(await recent.evaluate(e=>getComputedStyle(e).animationName),'none');
  await page.screenshot({path:new URL('recent-unread-'+theme+'-phone.png',out).pathname});
  await recent.click();await page.waitForFunction(()=>!document.querySelector('#recent-toggle').classList.contains('has-updates'));await page.locator('#close-recent').click();await page.emulateMedia({reducedMotion:'no-preference'});
 }
 if(process.env.INFUSER_TOUCH!=='1'){
  await page.setViewportSize({width:1500,height:1000});await page.mouse.move(1,1);await page.waitForTimeout(300);const before=await card().boundingBox();
  await card().hover();await page.waitForTimeout(300);const after=await card().boundingBox();
  assert(after.width>before.width&&after.height>before.height);assert(Math.abs((before.x+before.width/2)-(after.x+after.width/2))<1);assert(Math.abs((before.y+before.height/2)-(after.y+after.height/2))<1);
  await page.emulateMedia({reducedMotion:'reduce'});assert.equal(await card().evaluate(e=>getComputedStyle(e).transform),'none');await page.emulateMedia({reducedMotion:'no-preference'});
 }
 await page.emulateMedia({reducedMotion:'reduce'});await open();await dialog.getByRole('button',{name:'React: Amazing',exact:true}).click();assert.equal(await page.locator('.reaction-burst').count(),0);await page.locator('#close-reaction').click();await close();await page.emulateMedia({reducedMotion:'no-preference'});
 // Actual description/text are rendered as text, not interpreted as markup.
 game.description='<img src=x onerror=alert(1)> '+('Long description. '.repeat(80));game.title='An exceptionally long game title that should remain usable on a narrow phone';
 await page.reload();await page.locator('.game-open').first().waitFor();await page.setViewportSize({width:320,height:640});await open();assert.equal(await dialog.locator('.game-description img').count(),0);assert(await dialog.evaluate(e=>e.scrollWidth<=e.clientWidth));await close();
 // Polling preserves loaded artwork, focus, and reading position in details.
 await open();await page.evaluate(()=>{window.detailImage=document.querySelector('.detail-art img');document.querySelector('.detail-creator .creator').focus();document.querySelector('#game-detail').scrollTop=200;});
 game.reactions.push({...friend,emoji:'🚀',text:'New reaction',updated:now+2});
 await page.evaluate(()=>refreshTestLibrary());assert(await page.evaluate(()=>document.querySelector('.detail-art img')===detailImage));assert(await dialog.locator('.detail-creator .creator').evaluate(e=>e===document.activeElement));assert(await dialog.evaluate(e=>e.scrollTop>=200));await close();
 // Admin visibility moves a game between the shared shelves.
 await page.setViewportSize({width:1280,height:900});await page.locator('#hidden-games').evaluate(e=>e.open=false);await open();await dialog.locator('.play-menu summary').click();await dialog.getByRole('button',{name:'Make invisible',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('#visible-games .game').length===2);await close();await page.waitForFunction(()=>document.querySelector('#hidden-games > summary')===document.activeElement);
 await page.locator('#hidden-games').evaluate(e=>e.open=true);await page.locator('#hidden-game-list [data-app="firefly-drift"] .game-open').click();await dialog.locator('.play-menu summary').click();await dialog.getByRole('button',{name:'Make visible',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('#visible-games .game').length===3);await close();
 // Presence portrait owns the existing profile options, including theme/install/notifications.
 await page.locator('#account-trigger').click();await page.locator('#open-profile').click();await page.locator('input[name="avatar"][value="byte-gecko"]').check();await page.locator('#save-profile').click();await page.waitForFunction(()=>!document.querySelector('#profile-dialog').open);assert.equal(me.avatar,'byte-gecko');
 await page.locator('#add-game').click();await page.waitForFunction(()=>document.querySelector('#agent-prompt').value.includes('TEST-ONLY'));await page.locator('#close-add-game').click();
 await page.emulateMedia({reducedMotion:'reduce'});assert.equal(await card().evaluate(e=>getComputedStyle(e).transitionDuration),'0s');
 await page.emulateMedia({forcedColors:'active'});await open();assert(await dialog.isVisible());await close();await page.emulateMedia({forcedColors:'none'});
 // The same detail UI works read-only in operator inspection.
 me.inspecting=true;me.admin=false;await page.reload();await page.locator('.game-open').first().waitFor();await open();assert(await dialog.locator('.actions > button').isDisabled());assert(await dialog.locator('.creator').isDisabled());assert.equal(await dialog.locator('.reaction-choice:disabled').count(),6);await close();
 unauthorized=true;await page.reload();await page.locator('#welcome').waitFor();assert(await page.locator('.google img').isVisible());
 assert.deepEqual(errors,[]);
 console.log('PASS: shared details/cards, both themes at desktop/tablet/320/390, launch/guest/join, pending-art transitions, author search, 28-day attenuated reaction stacks, positive reactions with optional notes and bursts, unread Recently, centered card scaling, tags, activity targets, profile, visibility, text safety, inspection, landing, reduced motion and forced colors.');
} finally {await browser.close();}
