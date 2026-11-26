import {ordinaryThemeCalendar} from './support/theme_calendar.mjs';
// Shared Library presentation/interaction regression with an isolated mock API.
// Set INFUSER_PLAYWRIGHT and INFUSER_CHROMIUM when using an existing browser install.
import {readFile,mkdir} from 'node:fs/promises';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const root=new URL('../',import.meta.url),out=new URL('.local/library-detail-review/',root);
await mkdir(out,{recursive:true});
const avatars=[...String(await readFile(new URL('src/portal/avatars.rs',root))).matchAll(/avatar!\("([a-z-]+)", "([^"]+)"\)/g)].map(([,id,name])=>({id,name}));
const illustrated=[['🐦‍⬛','Amazing atmosphere','atmosphere'],['🍵','Cozy world','cozy'],['💎','A little gem','gem'],['🧩','Clever design','clever'],['🎢','Wild ride','wild'],['🪄','Full of surprises','surprises'],['🎨','Beautiful art','art'],['🎵','Great soundtrack','music'],['😄','Made me smile','smile'],['🤝','Fun together','together'],['🔥','On fire!','fire'],['👻','So scary!','spooky'],['🏆','Instant classic','trophy'],['🎉','Total blast','celebrate']];
const me={name:'KwamiKwami',player_id:'one',admin:true,avatar:'loot-raccoon',avatars};
const author={id:'one',name:me.name,avatar:me.avatar},friend={id:'two',name:'OrbitFox',avatar:'ember-raven'};
const now=Math.floor(Date.now()/1000),tags=['3d','action','adventure','arcade','chill','co-op','exploration','sandbox','shooter','simulation','survival'];
const game={can_manage_visibility:true,can_moderate_reactions:true,id:'firefly-drift',title:'Firefly Drift',description:'A quiet flight through a glowing forest.',author,updated:now-60,card_art_url:'/test-art/firefly',url:'https://library.test/games/firefly-drift',tags:['3d','arcade','chill'],can_edit_tags:true,players:[],join_targets:[{name:'OrbitFox',session_id:'session',player_id:'two'}],reactions:[{...friend,emoji:'👏',text:'Love the atmosphere.',updated:now,mine:false},{...author,emoji:'⭐',text:'',updated:now-10,mine:true}]};
const apps=[game,{...game,id:'nocomgame',title:'NoCOMgame',description:null,author:friend,card_art_url:null,tags:['3d','sandbox','action','survival','exploration'],reactions:[]},{...game,id:'event-horizon',title:'Event Horizon',author:friend,card_art_url:'/test-art/event',reactions:[]},{...game,id:'dodge',title:'Dodge the Creeps',author:null,preview:true,card_art_url:null,reactions:[]},{...game,id:'lantern',title:'Lantern Lake',author:null,preview:true,card_art_url:null,reactions:[]}];
for(const app of apps){app.update_id=app.id+'-release-a';app.update_reactions={mine:null,counts:{}};}
const recentReactions=n=>Array.from({length:n},(_,i)=>({id:'buzz-'+i,name:'Player '+i,emoji:['⭐','👍','🚀','❤️','👏','💡'][i%6],updated:now-i-2,text:'',mine:false}));
apps[1].card_art_pending=true;apps[1].reactions=recentReactions(8);apps[2].reactions=recentReactions(12);apps[4].reactions=recentReactions(18);
let launches=[],writes=[],reads=0,unauthorized=false,failModeration=false,failVisibility=false,releaseVisibility,failAuthors=false,failReaction=false,releaseReaction;
let houseSettings={topic_set:false,quiet_hours:null,profiles:[author,friend],images:JSON.parse(await readFile(new URL('web/assets/house-calls/manifest.json',root))).map(image=>({...image,enabled:false,tag:'',message:'',cooldown_minutes:5,profiles:[]}))};
const browser=await chromium.launch({headless:true,executablePath:process.env.INFUSER_CHROMIUM});
try {
 const context=await browser.newContext({hasTouch:process.env.INFUSER_TOUCH==='1',viewport:{width:1500,height:1000},serviceWorkers:'block',permissions:['clipboard-read','clipboard-write']});
 await context.route('https://library.test/**',async route=>{
  const request=route.request(),path=new URL(request.url()).pathname,method=request.method();let data;
  if(path==='/api/presence/input')return route.fulfill({json:{ok:true}});
  if(method!=='GET')writes.push({path,body:request.postDataJSON()});
  if(path==='/api/me')return route.fulfill({status:unauthorized?401:200,json:unauthorized?{error:'Sign in'}:me});
  if(path==='/api/library'){reads++;data={apps,friends:[author,friend],tags:tags.map(id=>({id,label:id==='3d'?'3D':id[0].toUpperCase()+id.slice(1),count:apps.filter(g=>g.tags.includes(id)).length}))};}
  else if(path==='/api/chat')data={messages:[]};
  else if(path==='/api/screenshots')data={screenshots:[{id:'shot'}]};
  else if(path==='/api/screenshots/shot')data={image:'data:image/webp;base64,'+(await readFile(new URL('assets/avatars/loot-raccoon-128.webp',root))).toString('base64')};
  else if(path==='/api/client-state')data={version:'review',game_open:false};
  else if(path==='/api/admin/house-calls'){
   if(method==='PUT'){const body=request.postDataJSON();houseSettings={...houseSettings,images:body.images,quiet_hours:body.quiet_hours,topic_set:body.topic_url===undefined?houseSettings.topic_set:!!body.topic_url};data={ok:true};}else data=houseSettings;
  }
  else if(path==='/api/admin/game-authors')data={games:apps.map(g=>({id:g.id,title:g.title,authors:g.authors??(g.author?[g.author]:[])})),members:[author,friend]};
  else if(path.endsWith('/authors')){if(failAuthors)return route.fulfill({status:502,body:''});const app=apps.find(g=>g.id===path.split('/')[4]);app.authors=request.postDataJSON().authors.map(id=>[author,friend].find(a=>a.id===id));app.author=app.authors[0];data={authors:app.authors};}
  else if(path==='/api/admin/members')data={members:{}};
  else if(path==='/api/admin/games')data={games:[]};
  else if(path==='/api/admin/credentials')data={credentials:[]};
  else if(path==='/api/admin/peer-health')data={sessions:[]};
  else if(path==='/api/profile'&&method==='GET')data={users:[]};
  else if(path==='/api/profile'){me.avatar=request.postDataJSON().avatar;author.avatar=me.avatar;data={avatar:me.avatar};}
  else if(path.endsWith('/launch')){launches.push(request.postDataJSON());data={state:'ready',url:'https://library.test/game-fixture'};}
  else if(path.endsWith('/visibility')){if(failVisibility){await new Promise(resolve=>{releaseVisibility=resolve;});return route.fulfill({status:504,body:'Gateway Timeout'});}const app=apps.find(g=>g.id===path.split('/')[4]);app.preview=!request.postDataJSON().listed;data={listed:!app.preview};}
  else if(/^\/api\/admin\/apps\/[^/]+\/reactions\//.test(path)){
   if(failModeration)return route.fulfill({status:400,json:{error:'Please retry deletion.'}});
   const app=apps.find(g=>g.id===path.split('/')[4]);app.reactions=app.reactions.filter(r=>r.id!==path.split('/')[6]);data={ok:true};
  }
  else if(path.includes('/updates/') && path.endsWith('/reaction')){
   if(failReaction){await new Promise(resolve=>{releaseReaction=resolve;});return route.fulfill({status:503,json:{error:'Please retry reaction.'}});}
   if(path.split('/')[5]!==game.update_id)return route.fulfill({status:409,json:{error:'A newer update is available.'}});
   const {emoji}=request.postDataJSON(),votes=game.update_reactions;
   if(votes.mine)votes.counts[votes.mine]--;
   votes.mine=emoji;votes.counts[emoji]=(votes.counts[emoji]||0)+1;data={ok:true};
  }
  else if(path.endsWith('/reaction')){game.reactions=game.reactions.filter(r=>!r.mine);if(method==='PUT')game.reactions.push({...author,...request.postDataJSON(),updated:now+1,mine:true});data={ok:true};}
  else if(path.endsWith('/tags')){game.tags=request.postDataJSON().tags;data={ok:true};}
  else if(path==='/api/my-games')data={api_url:'https://library.test',app_id:'new-game',setup_code:'TEST-ONLY',expires_at:now+10800};
  else if(path==='/api/feedback')data={available:false};
  else if(path==='/game-fixture')return route.fulfill({contentType:'text/html',body:'<title>Test game</title>'});
  else {
   let file,type;
   if(path.startsWith('/assets/house-calls/')){file='web'+path;type='image/png';}
   else if(path==='/test-art/firefly'){file='docs/design-explorations/card-art-guide/firefly-drift.webp';type='image/webp';}
   else if(path==='/test-art/event'){file='assets/avatars/event-horizon.png';type='image/png';}
   else if(path.startsWith('/assets/reactions/')){file=path.slice(1);type='image/webp';}
   else if(path.startsWith('/assets/avatars/')){const [,id,size]=path.match(/\/avatars\/([^/]+)\/(\d+)\.webp$/);file=`assets/avatars/${id}-${size}.webp`;type='image/webp';}
   else if(path==='/assets/card-generating.webp'){file='assets/card-fallback/generating.webp';type='image/webp';}
   else if(path==='/assets/google-signin.png'){file='assets/google/sign-in-with-google-pill.png';type='image/png';}
   else if(path.startsWith('/assets/')){file='web/'+path.slice(8);type=path.endsWith('.js')?'text/javascript':path.endsWith('.svg')?'image/svg+xml':path.endsWith('.woff2')?'font/woff2':'text/css';}
   else if(path==='/sw.js')return route.fulfill({status:404,body:''});
   else {file='web/index.html';type='text/html';}
   let body=await readFile(new URL(file,root));
   if(file==='web/themes.js')body=ordinaryThemeCalendar(body.toString());
   if(file==='web/clubhouse.js')body=body.toString()+'\nwindow.refreshTestLibrary=refresh;window.testRecentAge=recentAge;window.refreshTestAdmin=adminRefresh;';
   return route.fulfill({body,contentType:type});
  }
  return route.fulfill({json:data});
 });
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto('https://library.test/library');await page.locator('#visible-games .game').first().waitFor();
 for(const gatewayStatus of [502,503,504]){
   await page.route('**/api/library',r=>r.fulfill({status:gatewayStatus,body:''}));
   await page.evaluate(()=>refreshTestLibrary());
   assert.equal(await page.locator('#notice').textContent(),'Infuser is restarting. Try again in a moment.');
   await page.unroute('**/api/library');
 }
 const card=()=>page.locator('#visible-games [data-app="firefly-drift"]'),dialog=page.locator('#game-detail');
 async function close(){await page.locator('#close-game-detail').click();await page.waitForFunction(()=>!document.querySelector('#game-detail').open);}
 async function open(){await card().locator('.game-open').click();await dialog.waitFor();}
 async function popup(action){const opening=page.waitForEvent('popup');await action();const tab=await opening;await tab.waitForURL('**/game-fixture');await tab.close();}
 await open();assert.equal(await dialog.locator('.detail-input-methods').count(),0);
 game.input_methods=['keyboard_mouse','gamepad'];await page.evaluate(()=>refreshTestLibrary());
 assert.equal(await dialog.locator('.detail-input-methods').textContent(),'Playable with Keyboard + mouse · Gamepad');
 assert.equal(await card().locator('.detail-input-methods').count(),0);
 const previousViewport=page.viewportSize();
 await page.setViewportSize({width:320,height:640});
 assert(await dialog.locator('.detail-input-methods').evaluate(row=>row.scrollWidth<=row.clientWidth));
 await page.setViewportSize(previousViewport);
 delete game.input_methods;await page.evaluate(()=>refreshTestLibrary());await close();
 // Admin credits reorder without granting ownership; compact portraits keep the dock size.
 const originalCreator=await card().locator('.creator').boundingBox();
 // A JSON service error from launch keeps its actionable server message.
 const capacityError='game capacity is full; retry after another game becomes idle';
 await page.route('**/api/apps/firefly-drift/launch',r=>r.fulfill({status:503,json:{error:capacityError}}));
 await card().getByRole('button',{name:'Play',exact:true}).click();
 await page.getByText(capacityError,{exact:true}).waitFor();
 assert.equal(await page.locator('#notice').textContent(),capacityError);
 await page.unroute('**/api/apps/firefly-drift/launch');

 await page.locator('#account-trigger').click();await page.getByRole('button',{name:'Admin tools',exact:true}).click();
 const editor=page.locator('#game-authors');await page.locator('#admin summary').filter({hasText:'Game authors'}).click();await editor.getByLabel('Game to credit').selectOption(game.id);
 await editor.getByLabel('Author to add').selectOption(friend.id);await editor.getByRole('button',{name:'Add',exact:true}).click();
 await editor.getByLabel('Move earlier '+friend.name,{exact:true}).click();
 assert.deepEqual(await editor.locator('.author-person > span:last-child').allTextContents(),[friend.name,author.name]);
 await page.evaluate(()=>refreshTestLibrary());assert.equal(await editor.locator('.author-person').count(),2,'polling preserves draft');
 failAuthors=true;await editor.getByRole('button',{name:'Save authors',exact:true}).click();await editor.getByText('Infuser is restarting. Try again in a moment.',{exact:true}).waitFor();
 assert.deepEqual(await editor.locator('.author-person > span:last-child').allTextContents(),[friend.name,author.name]);failAuthors=false;
 await editor.getByRole('button',{name:'Save authors',exact:true}).click();await editor.getByText('Authors saved.',{exact:true}).waitFor();
 assert.deepEqual(writes.at(-1).body.authors,[friend.id,author.id]);
 const house=page.locator('#house-calls');await house.waitFor({state:'attached'});
 assert.equal(await house.evaluate(el=>el.closest('details').open),false,'House calls starts folded like other admin regions');
 await page.locator('#admin summary').filter({hasText:'House calls'}).click();
 await house.getByRole('button',{name:'Replace',exact:true}).click();await house.getByLabel('New ntfy topic URL').fill('https://ntfy.example/private-test-topic');
 const call=house.locator('form>fieldset').nth(1),quiet=house.locator('form>fieldset').first();
 await call.getByLabel('Enabled',{exact:true}).check();await call.getByLabel('ntfy tag',{exact:true}).fill('family_time');await call.locator('.house-call-permissions > summary').click();await call.getByLabel(friend.name,{exact:true}).check();await call.locator('.house-call-permissions > summary').click();
 await quiet.getByLabel('Enabled',{exact:true}).check();await quiet.getByLabel('From',{exact:true}).fill('22:00');await quiet.getByLabel('Until',{exact:true}).fill('08:00');await quiet.getByLabel('Time zone',{exact:true}).fill('America/Los_Angeles');
 const cooldown=call.getByLabel('Cooldown (minutes)');await cooldown.fill('');
 const houseWrites=writes.filter(w=>w.path==='/api/admin/house-calls').length;
 await house.getByRole('button',{name:'Save house calls'}).click();
 assert(await cooldown.evaluate(el=>el.validity.valueMissing),'blank cooldown is required, not coerced to zero');
 assert.equal(writes.filter(w=>w.path==='/api/admin/house-calls').length,houseWrites);
 await cooldown.fill('5');
 await house.getByRole('button',{name:'Save house calls'}).click();await house.getByText('House calls saved.',{exact:true}).waitFor();
 assert.equal(await house.getByLabel('New ntfy topic URL').inputValue(),'');assert(await house.getByText('ntfy topic URL: set',{exact:true}).isVisible());
 assert.deepEqual(houseSettings.quiet_hours,{start:'22:00',end:'08:00',timezone:'America/Los_Angeles'});assert.deepEqual(houseSettings.images[0].profiles,[friend.id]);
 assert(!JSON.stringify(houseSettings).includes('private-test-topic'),'server view never returns topic URL');
 assert((await house.locator('.house-call-row').first().boundingBox()).height<=76,'desktop settings fit one compact row per image');
 await house.scrollIntoViewIfNeeded();await page.screenshot({path:new URL('house-call-admin.png',out).pathname});
 for(const width of [900,390,320]){
   await page.setViewportSize({width,height:900});await house.scrollIntoViewIfNeeded();
   assert(await house.evaluate(el=>el.scrollWidth<=el.clientWidth),'house-call settings fit at '+width+'px');
   await page.screenshot({path:new URL('house-call-admin-'+width+'.png',out).pathname});
 }
 await page.setViewportSize({width:1500,height:1000});
 // The actual profile dropdown must remain legible in every shipped theme.
 await call.locator('.house-call-permissions > summary').click();
 for(const theme of ['nightglass','cartridge-club','field-notes','funny-pages','castle-keep']){
   await page.evaluate(theme=>InfuserThemes.select(theme),theme);
   const contrast=await call.locator('.house-call-profiles').evaluate(list=>{
     const canvas=document.createElement('canvas');canvas.width=canvas.height=1;const ctx=canvas.getContext('2d');
     const luminance=color=>{ctx.clearRect(0,0,1,1);ctx.fillStyle=color;ctx.fillRect(0,0,1,1);const rgb=[...ctx.getImageData(0,0,1,1).data].slice(0,3).map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;});return rgb[0]*.2126+rgb[1]*.7152+rgb[2]*.0722;};
     const bg=luminance(getComputedStyle(list).backgroundColor);
     return [...list.querySelectorAll('label,legend')].map(el=>{const fg=luminance(getComputedStyle(el).color);return {text:el.textContent,ratio:(Math.max(fg,bg)+.05)/(Math.min(fg,bg)+.05)};});
   });
   for(const item of contrast)assert(item.ratio>=4.5,theme+' profile list contrast: '+JSON.stringify(item));
   await call.scrollIntoViewIfNeeded();await page.screenshot({path:new URL('house-call-profiles-'+theme+'.png',out).pathname});
 }
 await page.evaluate(()=>InfuserThemes.select('nightglass'));
 await call.locator('.house-call-permissions > summary').click();
 houseSettings.profiles.push({id:'three',name:'New profile'});
 await page.evaluate(()=>refreshTestAdmin());
 await call.locator('.house-call-permissions > summary').click();
 assert.equal(await call.getByLabel('New profile',{exact:true}).count(),1,'admin refresh includes newly added profiles without reopening');
 await call.locator('.house-call-permissions > summary').click();
 await call.getByLabel('ntfy tag',{exact:true}).fill('unsaved_tag');
 await page.evaluate(()=>refreshTestAdmin());
 assert.equal(await call.getByLabel('ntfy tag',{exact:true}).inputValue(),'unsaved_tag','admin refresh preserves unsaved house-call settings');
 await house.getByRole('button',{name:'Clear',exact:true}).click();await house.getByRole('button',{name:'Save house calls'}).click();await house.getByText('ntfy topic URL: not set',{exact:true}).waitFor();
 await page.locator('#close-admin').click();
 assert.equal((await card().locator('.creator').boundingBox()).width,originalCreator.width);
 assert.equal(await card().locator('.author-stack .avatar').count(),2);
 const badges=await card().locator('.author-stack .avatar').evaluateAll(nodes=>nodes.map(node=>{const r=node.getBoundingClientRect();return {x:r.x,y:r.y};}));
 assert.equal(badges[0].y,badges[1].y,'author badges overlap horizontally');
 assert(Math.abs(badges[1].x-badges[0].x-10)<.1,'author spacing is ten pixels');
 await open();assert.deepEqual(await dialog.locator('.detail-authors strong').allTextContents(),[friend.name,author.name]);await close();
 for(const width of [320,900]){
   await page.setViewportSize({width,height:900});
   for(const theme of ['nightglass','cartridge-club']){
     await page.evaluate(theme=>{document.documentElement.dataset.theme=theme;},theme);
     await card().scrollIntoViewIfNeeded();await card().screenshot({path:new URL(`authors-${theme}-${width}-card.png`,out).pathname});
     await open();assert.deepEqual(await dialog.locator('.detail-authors strong').allTextContents(),[friend.name,author.name]);
     assert(await dialog.locator('.detail-authors').evaluate(e=>e.scrollWidth<=e.clientWidth+1));
     await page.screenshot({path:new URL(`authors-${theme}-${width}-detail.png`,out).pathname});await close();
   }
 }
 await page.setViewportSize({width:1500,height:1000});await page.evaluate(()=>{document.documentElement.dataset.theme='nightglass';});
 // A short name sits flush with Play. A wrapped name grows upward from the same edge.
 const savedTitle=game.title;
 await page.setViewportSize({width:320,height:844});
 for(const theme of ['nightglass','cartridge-club']){
   await page.evaluate(t=>InfuserThemes.select(t),theme);
   for(const [title,wraps] of [['Go',false],['Firefly Drift and the Mystery of Lantern Lake',true]]){
     game.title=title;await page.evaluate(()=>refreshTestLibrary());
     const edges=await card().evaluate(c=>['.game-heading','.actions > button','.play-menu > summary'].map(s=>{
       const {top,bottom}=c.querySelector(s).getBoundingClientRect();return {top,bottom};
     }));
     const [plate,...controls]=edges;
     for(const control of controls){
       assert(Math.abs(plate.bottom-control.bottom)<1,`${theme}: title and Play share a bottom edge`);
       assert(wraps?plate.top<control.top-1:Math.abs(plate.top-control.top)<1,`${theme}: ${title} height`);
     }
     await card().screenshot({path:new URL(`card-alignment-${theme}-${wraps?'two':'one'}.png`,out).pathname});
   }
 }
 game.title=savedTitle;await page.evaluate(()=>refreshTestLibrary());
 await page.setViewportSize({width:1500,height:1000});await page.evaluate(()=>InfuserThemes.select('nightglass'));
 // Search finds a secondary author, then return to the original fixture for existing journeys.
 await page.locator('#game-search').fill(author.name);await page.keyboard.press('Escape');assert(await card().isVisible());await page.locator('#game-search').fill('');
 await page.locator('#account-trigger').click();await page.getByRole('button',{name:'Admin tools',exact:true}).click();
 await editor.getByLabel('Game to credit').selectOption(game.id);
 for(const width of [320,1500]){await page.setViewportSize({width,height:1000});assert(await editor.evaluate(e=>e.scrollWidth<=e.clientWidth+1));await page.screenshot({path:new URL(`authors-editor-${width}.png`,out).pathname});}
 await editor.getByLabel('Remove '+friend.name,{exact:true}).click();await editor.getByRole('button',{name:'Save authors',exact:true}).click();await editor.getByText('Authors saved.',{exact:true}).waitFor();await page.locator('#close-admin').click();
 delete game.authors;game.author=author;
 // Active player badges are separate from reactions and do not recreate covers.
 game.players=[{...author},{...author},{...friend},{...friend,guest:true,player_id:'guest-one'},{...friend,guest:true,player_id:'guest-two'},{id:'three',name:'Nova',avatar:'circuit-fox'},{id:'four',name:'Sky',avatar:'moss-knight'}];
 await page.evaluate(()=>refreshTestLibrary());
 await open();const roster=dialog.getByRole('region',{name:'Playing now'});
 assert.equal(await roster.locator('li').count(),5);assert(await roster.getByRole('button',{name:'Join OrbitFox',exact:true}).isVisible());
 await close();
 const live=()=>card().locator('.game-live');
 assert.equal(await live().locator('summary .game-player').count(),3);
 assert.equal(await live().locator('.game-player-more').textContent(),'+2');
 assert.equal(await live().locator('li').count(),5,'duplicate signed-in sessions share one badge');
 assert(await live().getByText('OrbitFox’s guests (2)',{exact:true}).count());
 assert(await live().locator('img[src="/assets/avatars/loot-raccoon/128.webp"]').count());
 await page.evaluate(()=>{window.liveBadge=document.querySelector('.game-live');window.liveCover=document.querySelector('[data-app="firefly-drift"] .game-art img');});
 await page.evaluate(()=>refreshTestLibrary());
 assert(await live().evaluate(e=>e===liveBadge),'unchanged polling retains badges and animations');
 await live().locator('summary').click();assert(await live().locator('.game-live-menu').isVisible());assert(await dialog.isHidden());
 assert.equal(await live().getByRole('button',{name:'Join OrbitFox',exact:true}).count(),1);
 assert.equal(await live().getByRole('button').count(),1,'non-joinable players and guests have no Join action');
 await popup(()=>live().getByRole('button',{name:'Join OrbitFox',exact:true}).click());
 assert.deepEqual(launches.at(-1),{guest:false,intent:{kind:'join_friend',session_id:'session',player_id:'two'}});
 const normalTargets=[...game.join_targets];
 game.join_targets.push({name:'Guest',guest:true,session_id:'guest-session',player_id:'guest-one'});await page.evaluate(()=>refreshTestLibrary());
 await live().locator('summary').click();await popup(()=>live().getByRole('button',{name:'Join OrbitFox’s guest',exact:true}).click());
 assert.deepEqual(launches.at(-1).intent,{kind:'join_friend',session_id:'guest-session',player_id:'guest-one'},'guest join uses the guest identity, never its owner');
 await live().locator('summary').click();
 await live().getByRole('button',{name:'Join OrbitFox',exact:true}).evaluate(e=>window.staleJoin=e);
 const beforeStaleJoin=launches.length;game.join_targets=[];await page.evaluate(()=>refreshTestLibrary());
 assert.equal(await live().getByRole('button').count(),0,'availability updates remove stale Join options');
 await page.evaluate(()=>staleJoin.click());assert.equal(launches.length,beforeStaleJoin,'stale click never falls back to ordinary Play');
 assert(await page.locator('#notice').getByText('That session is no longer available to join.',{exact:true}).isVisible());
 game.join_targets=normalTargets;game.native_only=true;await page.evaluate(()=>refreshTestLibrary());
 assert.equal(await live().locator('button').count(),0,'native launches that cannot carry a join intent offer no shortcut');
 game.native_only=false;game.play_unavailable='Unavailable while visiting';await page.evaluate(()=>refreshTestLibrary());assert.equal(await live().locator('button').count(),0);
 delete game.play_unavailable;await page.evaluate(()=>refreshTestLibrary());await live().locator('summary').click();

 game.players=game.players.filter(p=>p.id!=='four');await page.evaluate(()=>refreshTestLibrary());
 assert(await live().evaluate(e=>e.open),'player changes keep the open list usable');
 assert.equal(await live().locator('li').count(),4);
 assert(await card().locator('.game-art img').evaluate(e=>e===liveCover),'presence changes retain mounted artwork');
 await page.keyboard.press('Escape');assert.equal(await live().evaluate(e=>e.open),false);
 for(const theme of ['nightglass','cartridge-club'])for(const width of [1280,390,320]){
  await page.evaluate(t=>InfuserThemes.select(t),theme);await page.setViewportSize({width,height:900});
  await card().scrollIntoViewIfNeeded();
  const cardBox=await card().boundingBox(),badgeBox=await live().locator('summary').boundingBox();
  assert(badgeBox.x>cardBox.x+cardBox.width/2&&badgeBox.y<cardBox.y+30,'badges sit at the upper right');
  assert(await card().evaluate(e=>{const badges=e.querySelector('.game-live').getBoundingClientRect();return [...e.querySelectorAll('.tag-strip,.tag-more:not([hidden])')].every(tag=>tag.getBoundingClientRect().right<=badges.left);}),'tags do not overlap the player badges');
  await live().locator('summary').click();const panelBox=await live().locator('.game-live-menu').boundingBox();assert(panelBox.x>=0&&panelBox.x+panelBox.width<=width,'full player list fits viewport');
  await page.screenshot({path:new URL(`players-${theme}-${width}.png`,out).pathname});
  await page.keyboard.press('Escape');
 }
 await page.emulateMedia({reducedMotion:'reduce'});
 assert.equal(await live().locator('summary .game-player').first().evaluate(e=>getComputedStyle(e,'::before').animationName),'none');
 await page.emulateMedia({reducedMotion:'no-preference'});
 game.players=[];await page.evaluate(()=>refreshTestLibrary());assert.equal(await live().count(),0,'badges disappear when sessions end');
 await page.setViewportSize({width:1500,height:1000});
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
 // Search previews real gamers, then offers exact author selection or game details.
 const search=page.locator('#game-search'),suggestions=page.locator('#search-options');
 assert.equal(await search.getAttribute('placeholder'),'Find a game…');
 await search.focus();await search.click();
 assert.equal(await suggestions.getByRole('option').count(),2);
 assert(await suggestions.getByRole('option').first().textContent().then(t=>t.includes('Gamer')));
 await search.press('Escape');assert(await suggestions.isHidden());
 await search.press('ArrowDown');assert(await search.getAttribute('aria-activedescendant'));
 await search.press('Enter');assert.equal(await page.locator('#visible-games .game').count(),1);
 await search.fill('orbit');
 // A title containing the same handle must not leak into an exact author filter.
 const originalTitle=game.title;game.title='OrbitFox adventure';await page.evaluate(()=>refreshTestLibrary());
 assert.equal(await suggestions.getByRole('option').count(),2);
 await suggestions.getByRole('option',{name:'OrbitFox Gamer · View games',exact:true})[process.env.INFUSER_TOUCH==='1'?'tap':'click']();
 assert.equal(await page.locator('#visible-games .game').count(),2);
 assert.equal(await card().count(),0);
 await page.evaluate(()=>refreshTestLibrary());assert.equal(await page.locator('#visible-games .game').count(),2);
 game.title=originalTitle;await search.fill('');await page.evaluate(()=>refreshTestLibrary());
 await search.fill('fIrEf');const launchCount=launches.length;
 await suggestions.getByRole('option',{name:'Firefly Drift Game · KwamiKwami',exact:true})[process.env.INFUSER_TOUCH==='1'?'tap':'click']();
 assert(await dialog.isVisible());assert.equal(launches.length,launchCount);await close();
 await search.fill('nothing matches');assert(await suggestions.isHidden());
 await search.fill('orbit');await search.press('ArrowDown');await search.press('Escape');assert(await suggestions.isHidden());
 await search.click();await search.press('Tab');assert(await suggestions.isHidden());
 await search.fill('');await search.press('Escape');
 const dockInset=()=>pendingCard().evaluate(c=>c.getBoundingClientRect().bottom-c.querySelector('.game-dock').getBoundingClientRect().bottom);
 let emptyInset;
 // Only the last 28 days contribute; a busy card never grows past eight faces.
 for(const [n,faces] of [[0,0],[1,1],[8,4],[12,5],[18,6],[25,8],[50,8]]){
  apps[1].reactions=[...recentReactions(n),{id:'old',name:'Old',emoji:'⭐',updated:now-29*86400,text:''}];await page.evaluate(()=>refreshTestLibrary());
  assert.equal(await pendingCard().locator('.game-buzz span').count(),faces);
  if(!faces)emptyInset=await dockInset();
  else {
   const placed=await pendingCard().evaluate(c=>{
    const buzz=c.querySelector('.game-buzz'),dock=c.querySelector('.game-dock'),heading=c.querySelector('.game-heading');
    const b=buzz.getBoundingClientRect(),d=dock.getBoundingClientRect(),h=heading.getBoundingClientRect();
    const face=buzz.querySelector('span').getBoundingClientRect();
    return {beside:b.right<=h.left+1,below:face.top>=d.bottom-1,inset:c.getBoundingClientRect().bottom-d.bottom};
   });
   assert(placed.beside||placed.below,'reactions stay beside the title or just below it');
   if(placed.beside)assert(Math.abs(placed.inset-emptyInset)<1,'reactions beside the title keep the dock on its baseline');
   else assert(placed.inset>emptyInset+8,'reactions below the title sit under the dock');
  }
  if(faces===8)assert.equal(await pendingCard().locator('.game-buzz span').nth(1).evaluate(e=>getComputedStyle(e).marginLeft),'-15px','eight faces use the original cramped spacing');
 }
 apps[1].reactions=recentReactions(8);await page.evaluate(()=>refreshTestLibrary());
 assert.equal(new Set(await pendingCard().locator('.game-buzz span').allTextContents()).size,4);
 apps[1].reactions=recentReactions(8).map((r,i)=>({...r,emoji:illustrated[i%6][0]}));
 apps[2].reactions=recentReactions(18).map((r,i)=>({...r,emoji:illustrated[i%6][0]}));
 await page.evaluate(()=>refreshTestLibrary());
 await page.waitForFunction(()=>[...document.querySelectorAll('.game-buzz img')].every(i=>i.complete&&i.naturalWidth===256));
 assert.equal(await pendingCard().locator('.game-buzz img').count(),4);
 assert(await pendingCard().locator('.game-buzz span').evaluateAll(faces=>faces.every((face,i)=>{
  const style=getComputedStyle(face);
  return style.backgroundColor==='rgba(0, 0, 0, 0)'&&style.filter==='drop-shadow(rgba(0, 0, 0, 0.6) 4px 2px 0.5px)'&&style.textShadow==='none'&&(!i||Number(style.zIndex)<Number(getComputedStyle(faces[i-1]).zIndex));
 })),'leftmost reaction is on top; offset silhouette shadows without circular backings');

 await pendingCard().locator('.game-buzz').click();assert(await dialog.isVisible());assert(await page.evaluate(()=>document.activeElement.classList.contains('game-feedback-row')));await close();
 for(const theme of ['nightglass','cartridge-club']){
  await page.evaluate(t=>InfuserThemes.select(t),theme);
  for(const width of [1500,900,390,320]){
   await page.setViewportSize({width,height:width>650?1000:844});
   assert.equal(await page.locator('#visible-games .game').count(),3);assert.equal(await page.locator('#hidden-game-list .game').count(),2);
   await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),`${theme} ${width} page overflow`);
   assert(await page.evaluate(()=>{const brand=document.querySelector('.brand').getBoundingClientRect(),presence=document.querySelector('.presence-chat').getBoundingClientRect();return Math.abs((brand.top+brand.bottom)/2-(presence.top+presence.bottom)/2)<2;}),`${theme} ${width}: logo and presence stay on one header row`);
   assert.equal(await page.locator('#friends [data-player-id="one"]').count(),1);
   await page.locator('#hidden-games').evaluate(e=>e.open=true);
   await page.evaluate(()=>document.activeElement.blur());
   assert(await page.locator('#visible-games .game').evaluateAll(cards=>cards.every(card=>{
    const plate=card.querySelector('.game-heading'),row=plate.parentElement;
    const h=plate.getBoundingClientRect(),p=card.querySelector('.actions').getBoundingClientRect();
    const withinPlate=child=>{const r=child.getBoundingClientRect();return r.left>=h.left-1&&r.right<=h.right+1;};
    return h.right+8<=p.left&&Math.abs(h.bottom-p.bottom)<1&&
      h.left>=card.getBoundingClientRect().left&&[plate.querySelector('.game-open'),plate.querySelector('.creator')].every(withinPlate)&&
      parseFloat(getComputedStyle(plate).borderTopWidth)>0&&getComputedStyle(row).backgroundColor==='rgba(0, 0, 0, 0)';
   })),`${theme} ${width}: title and author share a bordered plate beside Play`);
   assert(await page.locator('#visible-games .game').evaluateAll(cards=>cards.every(card=>{
    const buzz=card.querySelector('.game-buzz'),play=card.querySelector('.actions'),heading=card.querySelector('.game-heading');
    if(!buzz)return true;
    const face=buzz.querySelector('span').getBoundingClientRect(),b=buzz.getBoundingClientRect(),p=play.getBoundingClientRect(),h=heading.getBoundingClientRect();
    const misses=box=>face.right<=box.left+1||face.left>=box.right-1||face.bottom<=box.top+1||face.top>=box.bottom-1;
    return (b.right<=h.left+1||face.top>=Math.max(h.bottom,p.bottom)-1)&&misses(h)&&misses(p);
   })),'reaction stacks sit beside the title or just below it, and never cover the controls');
   assert(await page.locator('#games .game').evaluateAll(cards=>cards.every(card=>{
    const buzz=card.querySelector('.game-buzz');if(!buzz)return true;
    const c=card.getBoundingClientRect(),face=buzz.firstElementChild.getBoundingClientRect(),hang=(face.bottom-c.bottom)/face.height;
    const dock=card.querySelector('.game-dock')?.getBoundingClientRect();
    const underPlate=!card.classList.contains('hidden-game')&&dock&&face.top>=dock.bottom-1&&face.bottom<=c.bottom+1;
    return face.left>=c.left&&face.left-c.left<=8&&((hang>=.15&&hang<=.25)||underPlate)&&buzz.getBoundingClientRect().height>=44;
   })),'emoji faces should hang 20% below the card or sit just under the title, stay inside its left edge, and retain a touch target');
   await search.fill('');await search.click();
   assert.equal(await suggestions.getByRole('option').count(),2);
   assert(await suggestions.evaluate(e=>{const r=e.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth&&e.scrollWidth<=e.clientWidth;}));
   if(width===390||width===1500)await page.screenshot({path:new URL(`${theme}-${width}-search.png`,out).pathname});
   await search.press('Escape');await search.blur();
   await page.screenshot({path:new URL(`${theme}-${width}.png`,out).pathname,fullPage:true});
   await open();await page.waitForFunction(()=>document.querySelector('.detail-art img').complete);
   assert.equal(await dialog.locator('.game-feedback-row').count(),2);
   assert.equal(await dialog.locator('.reaction-choice').count(),26);
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
 await dialog.locator('.creator').click();await page.locator('#chosen-reaction').click();await page.getByRole('radio',{name:'Great job',exact:true}).check();await page.locator('#reaction-text').fill('Fresh feedback');await page.locator('#reaction-form button[type="submit"]').click();
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

 // Expanded reactions retain large targets, labels and a bounded scrolling picker.
 for(const theme of ['nightglass','cartridge-club']){
  await page.evaluate(t=>InfuserThemes.select(t),theme);
  for(const width of [1280,390,320]){
   await page.setViewportSize({width,height:width===1280?900:568});await open();
   for(const [emoji,label,art] of [['😅','Tough challenge'],['⚡','Intense action'],['🧠','Made me think'],['🎯','Feels great'],['🧭','Fun to explore'],['🧲','One more round'],...illustrated]){
    await dialog.getByRole('button',{name:'React: '+label,exact:true}).click();
    const chosen=page.getByRole('button',{name:'Change emoji: '+label,exact:true});
    assert(await chosen.isVisible());
    assert(await page.locator('#emoji').isHidden());
    if(art){
     assert.equal(await chosen.locator('img').getAttribute('src'),'/assets/reactions/'+art+'.webp');
     assert.equal(await page.locator('.reaction-burst-hero img').getAttribute('src'),'/assets/reactions/'+art+'.webp');
    }
    await page.locator('#reaction-text').fill('');
    await page.getByRole('button',{name:'Send reaction',exact:true}).click();
    await page.waitForFunction(()=>!document.querySelector('#reaction-dialog').open);
    await page.waitForFunction(label=>document.querySelector('.reaction-choice[aria-label="React: '+label+'"]')?.getAttribute('aria-pressed')==='true',label);
    assert.equal(game.reactions.find(r=>r.mine).emoji,emoji);
   }
   await dialog.locator('.creator').click();
   await page.locator('#chosen-reaction').click();
   assert.equal(await page.getByRole('radio').count(),26);
   if(width===320)assert(await page.locator('#emoji').evaluate(e=>e.scrollHeight>e.clientHeight),'picker scrolls instead of shrinking emojis');
   await page.screenshot({path:new URL('expanded-reactions-'+theme+'-'+width+'.png',out).pathname});
   await page.locator('#close-reaction').click();await close();
  }
 }
 // Saved artwork is shared by the review, card stack and Recently. A failed
 // decorative image falls back to its persisted emoji without losing the label.
 const lastIllustrated='/assets/reactions/'+illustrated.at(-1)[2]+'.webp';
 await open();
 assert.equal(await dialog.locator('.feedback-emoji img').last().getAttribute('src'),lastIllustrated);
 await close();
 assert.equal(await card().locator('.game-buzz img').first().getAttribute('src'),lastIllustrated);
 await page.locator('#recent-toggle').click();
 assert(await page.locator('#feed .feed-emoji img[src="'+lastIllustrated+'"]').count()>0);
 await page.locator('#close-recent').click();
 await open();await dialog.getByRole('button',{name:'React: A little gem',exact:true}).click();
 const gemFace=page.locator('#chosen-reaction span');
 await gemFace.locator('img').dispatchEvent('error');assert.equal(await gemFace.textContent(),'💎');
 assert(await page.getByRole('button',{name:'Change emoji: A little gem',exact:true}).isVisible());
 await page.locator('#close-reaction').click();await close();
 // Obvious reaction controls: choose, add an optional kind note, or send emoji-only.
 await page.setViewportSize({width:390,height:844});await open();
 await dialog.getByRole('button',{name:'React: Exciting',exact:true}).click();
 assert(await page.getByRole('button',{name:'Change emoji: Exciting',exact:true}).isVisible());
 assert(await page.locator('#reaction-comment').isVisible());assert(await page.locator('#reaction-text').evaluate(e=>parseFloat(getComputedStyle(e).fontSize)>=22));
 await page.locator('#reaction-text').fill('');await page.getByRole('button',{name:'Send reaction',exact:true}).click();
 await page.waitForFunction(()=>document.querySelector('.reaction-choice[aria-label="React: Exciting"]').getAttribute('aria-pressed')==='true');
 assert.equal(game.reactions.find(r=>r.mine).text,'');assert.equal(game.reactions.find(r=>r.mine).emoji,'🚀');
 await dialog.locator('.creator').click();await page.locator('#remove-reaction').click();await page.waitForFunction(()=>!document.querySelector('#reaction-dialog').open);await page.evaluate(()=>refreshTestLibrary());
 await dialog.locator('.creator').click();assert(await page.locator('#reaction-comment').isHidden());
 await page.getByRole('radio',{name:'Loved it',exact:true}).check();await page.locator('#reaction-text').fill('I liked the glowing trail!');
 assert.equal(await page.locator('.reaction-burst:popover-open').count(),1);
 await page.getByRole('button',{name:'Add screenshot',exact:true}).click();
 await page.locator('#reaction-screenshots img').waitFor();
 await page.waitForFunction(()=>document.querySelector('#reaction-screenshots img')?.naturalWidth>0);
 await page.locator('#reaction-screenshots .screenshot-tile').click();
 assert(await page.getByRole('button',{name:'Change emoji: Loved it',exact:true}).isVisible());
 assert.equal(await page.locator('#reaction-text').inputValue(),'I liked the glowing trail!');
 await page.locator('#chosen-reaction').click();
 await page.getByRole('radio',{name:'Great job',exact:true}).check();
 assert(await page.locator('#emoji').isHidden());
 assert.equal(await page.locator('#reaction-text').inputValue(),'I liked the glowing trail!');
 assert(await page.locator('#reaction-draft').isVisible());
 await page.screenshot({path:new URL('reaction-composer-phone.png',out).pathname});
 await page.getByRole('button',{name:'Send reaction',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('#reaction-dialog').open);await close();
 assert.equal(game.reactions.find(r=>r.mine).screenshot,'shot');
 await open();await dialog.locator('.screenshot-tile').click();await page.locator('.screenshot-viewer img').waitFor();await page.keyboard.press('Escape');assert(await dialog.isVisible());await close();
 assert.deepEqual(await page.evaluate(()=>[0,60,3600,7200,86400,-20].map(age=>testRecentAge(100000-age,100000))),['just now','a minute ago','an hour ago','2 hours ago','a day ago','just now']);
 // A game review and an update vote are independent, even when another tab edits the review.
 game.updated=now+2;game.update_reactions.counts={'❤️':1};game.reactions=game.reactions.filter(r=>!r.mine);
 game.reactions.push({...author,emoji:'⭐',text:'Keep my note',screenshot:'shot',updated:now,mine:true});
 await page.evaluate(()=>refreshTestLibrary());await page.locator('#recent-toggle').click();
 const update=()=>page.locator('#feed').getByRole('button',{name:'React to Firefly Drift update',exact:true});
 const picker=page.locator('#feed-reaction-picker');
 assert.equal(await update().count(),1);
 assert(await update().locator('svg.feed-react-outline').isVisible());
 assert.equal(await update().locator('.feed-reaction-face').count(),0);
 await update().click();
 assert.equal(await picker.getByRole('button').count(),26);
 assert.equal(await picker.locator('input,textarea,small').count(),0);
 assert.equal(await picker.locator('img.reaction-image').count(),14,'all illustrated reactions are available');
 assert.equal(await picker.getByRole('button',{name:'React: Fun together',exact:true}).count(),1);
 await page.keyboard.press('Escape');assert(await picker.isHidden());assert(await page.locator('#recent-menu').evaluate(e=>e.open));assert(await update().evaluate(e=>e===document.activeElement));
 const panel=page.locator('#activity');
 await panel.evaluate(e=>e.style.maxHeight='160px');
 await update().click();
 // Constrain the emoji grid as on a short screen, exercising a real overflow scroll.
 await picker.evaluate(e=>{e.style.maxHeight='120px';e.scrollTop=40;});
 await page.waitForFunction(()=>document.querySelector('#feed-reaction-picker').scrollTop>0);
 assert(await picker.isVisible());
 await panel.evaluate(e=>e.scrollTop=e.scrollTop===0?40:0);
 await picker.waitFor({state:'hidden'});assert(await panel.isVisible());
 await page.waitForFunction(()=>document.querySelector('#feed .feed-react-toggle[data-update="firefly-drift-release-a"]').getAttribute('aria-expanded')==='false');
 await panel.evaluate(e=>e.style.removeProperty('max-height'));
 await update().click();
 const reactionsBefore=writes.length;failReaction=true;
 await picker.getByRole('button',{name:'React: Loved it',exact:true}).click();
 await page.waitForFunction(()=>document.querySelectorAll('#feed-reaction-picker button[aria-disabled="true"]').length===26);
 await page.evaluate(()=>refreshTestLibrary());
 assert(await picker.isVisible(),'polling keeps the picker open');
 await picker.getByRole('button',{name:'React: Great job',exact:true}).dispatchEvent('click');
 assert.equal(writes.length,reactionsBefore+1,'a pending save cannot be submitted twice');
 assert.equal(typeof releaseReaction,'function');releaseReaction();failReaction=false;
 await page.getByText('Please retry reaction.',{exact:true}).waitFor();
 await page.waitForFunction(()=>!document.querySelector('#feed-reaction-picker button[aria-disabled="true"]'));
 assert.equal(await page.locator('.update-confetti').count(),0);
 assert.equal(await picker.locator('[aria-pressed=true]').count(),0);
 game.reactions.find(r=>r.mine).text='Edited in another tab';
 await picker.getByRole('button',{name:'React: Loved it',exact:true}).click();
 await page.locator('.update-confetti:popover-open').waitFor();
 assert.deepEqual(writes.at(-1),{path:'/api/apps/firefly-drift/updates/'+game.update_id+'/reaction',body:{emoji:'❤️'}});
 await page.locator('#feed').getByRole('img',{name:'Loved it × 2',exact:true}).waitFor();
 await page.waitForFunction(()=>document.querySelector('.feed-react-toggle[data-update="firefly-drift-release-a"]')?.textContent==='❤️');
 assert.equal(await update().locator('svg.feed-react-outline').count(),0);
 assert(await picker.isHidden());assert(await page.locator('#recent-menu').evaluate(e=>e.open));assert(await dialog.isHidden());
 assert.equal(await page.locator('#notice.visible').count(),0,'a successful retry clears the earlier error');
 assert(await page.locator('.update-confetti-piece').count()>0);
 assert(await page.locator('.update-confetti-piece').evaluateAll(pieces=>new Set(pieces.map(piece=>piece.style.background)).size>=3));
 await page.screenshot({path:new URL('recent-reaction-confetti.png',out).pathname});
 await page.waitForFunction(()=>!document.querySelector('.update-confetti'));
 await update().click();await picker.getByRole('button',{name:'React: Nice!',exact:true}).focus();await page.keyboard.press('Enter');
 await page.waitForFunction(()=>document.querySelector('.feed-react-toggle[data-update="firefly-drift-release-a"]')?.textContent==='👍');
 assert(await update().evaluate(e=>e===document.activeElement),'saving returns keyboard focus to the reaction button');
 await page.emulateMedia({reducedMotion:'reduce'});
 await update().click();await picker.getByRole('button',{name:'React: Fun together',exact:true}).click();
 await page.waitForFunction(()=>document.querySelector('.feed-react-toggle[data-update="firefly-drift-release-a"] img')?.getAttribute('src')==='/assets/reactions/together.webp');
 assert.equal(game.update_reactions.mine,'🤝');assert.deepEqual(game.reactions.find(r=>r.mine),{...author,emoji:'⭐',text:'Edited in another tab',screenshot:'shot',updated:now,mine:true});assert.equal(await page.locator('.update-confetti').count(),0);
 for(const theme of ['nightglass','cartridge-club'])for(const width of [320,390,1500]){
  await page.setViewportSize({width,height:900});await page.evaluate(t=>InfuserThemes.select(t),theme);
  await update().click();
  assert(await picker.evaluate(e=>{const b=e.getBoundingClientRect();return b.left>=0&&b.right<=innerWidth&&b.top>=0&&b.bottom<=innerHeight&&e.scrollWidth<=e.clientWidth;}),'picker stays in the viewport without horizontal overflow');
  assert(await picker.getByRole('button').evaluateAll(buttons=>buttons.every(b=>b.getBoundingClientRect().height>=44)));
  await page.screenshot({path:new URL('recent-picker-'+theme+'-'+width+'.png',out).pathname});
  await page.keyboard.press('Escape');
 }
 // A publication arriving between opening the picker and submitting cannot inherit the vote.
 await update().click();game.update_id='firefly-drift-release-b';game.update_reactions={mine:null,counts:{}};
 await picker.getByRole('button',{name:'React: Loved it',exact:true}).click();
 await page.getByText('A newer update is available.',{exact:true}).waitFor();await picker.waitFor({state:'hidden'});
 assert.equal(game.update_reactions.mine,null);assert.equal(await page.locator('.update-confetti').count(),0);
 await update().click();assert.equal(await picker.locator('[aria-pressed=true]').count(),0);await page.keyboard.press('Escape');
 await page.setViewportSize({width:390,height:844});
 await page.locator('#close-recent').click();await page.emulateMedia({reducedMotion:'no-preference'});
 // Unread Recently animates only the icon, clears on opening,
 // and retains a static highlight with reduced motion.
 for(const theme of ['nightglass','cartridge-club']){
  await page.evaluate(t=>InfuserThemes.select(t),theme);game.updated=Math.max(now,game.updated)+1;await page.evaluate(()=>refreshTestLibrary());
  const recent=page.locator('#recent-toggle');assert(await recent.evaluate(e=>e.classList.contains('has-updates')));
  assert(await recent.evaluate(e=>e.hasAttribute('data-motion')&&getComputedStyle(e,'::before').animationName==='recent-halo'));
  await page.waitForFunction(()=>document.querySelector('#recent-toggle').classList.contains('wiggling'),{},{timeout:16000});
  assert.equal(await recent.locator('img.brand-icon').evaluate(e=>getComputedStyle(e).animationName),'recent-wiggle');
  assert(await recent.evaluate(e=>getComputedStyle(e,'::before').backgroundImage.includes('radial-gradient')));
  assert.equal(await recent.locator('img.brand-icon').count(),1);
  assert.equal(await recent.textContent(),'');
  await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,value:true});document.dispatchEvent(new Event('visibilitychange'));});
  assert(!await recent.evaluate(e=>e.hasAttribute('data-motion')||e.classList.contains('wiggling')),'hidden tabs stop both effects');
  await page.evaluate(()=>{delete document.hidden;document.dispatchEvent(new Event('visibilitychange'));});
  assert.equal(await page.locator('.brand-wordmark').evaluate(e=>getComputedStyle(e).animationName),'none');
  assert(await recent.evaluate(e=>e.getBoundingClientRect().width>=44&&e.getBoundingClientRect().height>=44));
  await page.emulateMedia({reducedMotion:'reduce'});assert.equal(await recent.evaluate(e=>getComputedStyle(e,'::before').animationName),'none');await page.waitForFunction(()=>!document.querySelector('#recent-toggle').hasAttribute('data-motion'));
  await page.screenshot({path:new URL('recent-unread-'+theme+'-phone.png',out).pathname});
  await recent.click();assert.equal(await page.locator('#feed time').count(),1);assert.equal(await page.locator('#feed .feed-row').first().locator('time').count(),1);await page.screenshot({path:new URL('recent-open-'+theme+'-phone.png',out).pathname});await page.waitForFunction(()=>!document.querySelector('#recent-toggle').classList.contains('has-updates'));await page.locator('#close-recent').click();await page.emulateMedia({reducedMotion:'no-preference'});
 }
 if(process.env.INFUSER_TOUCH!=='1'){
  await page.evaluate(()=>InfuserThemes.select('nightglass'));
  await page.setViewportSize({width:1500,height:1000});await page.mouse.move(1,1);await page.waitForTimeout(300);const before=await card().boundingBox();
  await card().hover();await page.waitForTimeout(300);const after=await card().boundingBox();
  assert(after.width>before.width&&after.height>before.height);assert(Math.abs((before.x+before.width/2)-(after.x+after.width/2))<1);assert(Math.abs((before.y+before.height/2)-(after.y+after.height/2))<1);
  await page.emulateMedia({reducedMotion:'reduce'});assert.equal(await card().evaluate(e=>getComputedStyle(e).transform),'none');await page.emulateMedia({reducedMotion:'no-preference'});
 }
 await page.emulateMedia({reducedMotion:'reduce'});await open();await dialog.getByRole('button',{name:'React: Cozy world',exact:true}).click();assert.equal(await page.locator('.reaction-burst').count(),0);await page.locator('#close-reaction').click();await close();await page.emulateMedia({reducedMotion:'no-preference'});
 // Admin moderation deletes the chosen reaction and note, with a recoverable error.
 const beforeModeration=[...game.reactions];await open();
 const target=dialog.locator('.game-feedback-row').filter({hasText:'Love the atmosphere.'});
 await target.locator('.feedback-actions summary').click();failModeration=true;
 await target.getByRole('button',{name:'Delete reaction',exact:true}).click();
 await target.getByRole('alert').getByText('Please retry deletion.',{exact:true}).waitFor();
 assert.equal(game.reactions.length,beforeModeration.length);failModeration=false;
 await target.locator('.feedback-actions summary').click();await target.getByRole('button',{name:'Delete reaction',exact:true}).click();
 await target.waitFor({state:'detached'});assert(!game.reactions.some(r=>r.id===friend.id));assert(game.reactions.some(r=>r.id===author.id));
 assert.equal(await page.locator('#feed').getByText('Love the atmosphere.',{exact:true}).count(),0,'removed reaction also leaves the activity feed');
 await close();game.reactions=beforeModeration;await page.evaluate(()=>refreshTestLibrary());
 me.admin=false;await page.reload();await page.locator('.game-open').first().waitFor();await open();assert.equal(await dialog.locator('.feedback-actions').count(),0);assert.equal(await dialog.locator('.game-admin-menu').count(),0);await close();
 me.admin=true;game.can_moderate_reactions=false;await page.reload();await page.locator('.game-open').first().waitFor();await open();assert.equal(await dialog.locator('.feedback-actions').count(),0,'server capability is required');await close();
 game.can_moderate_reactions=true;game.can_manage_visibility=false;await page.evaluate(()=>refreshTestLibrary());await open();assert.equal(await dialog.locator('.game-admin-menu').count(),0,'server visibility capability is required');await close();game.can_manage_visibility=true;await page.evaluate(()=>refreshTestLibrary());
 // Actual description/text are rendered as text, not interpreted as markup.
 game.description='<img src=x onerror=alert(1)> '+('Long description. '.repeat(80));game.title='An exceptionally long game title that should remain usable on a narrow phone';
 await page.reload();await page.locator('.game-open').first().waitFor();await page.setViewportSize({width:320,height:640});await open();assert.equal(await dialog.locator('.game-description img').count(),0);assert(await dialog.evaluate(e=>e.scrollWidth<=e.clientWidth));await close();
 // Polling preserves loaded artwork, focus, and reading position in details.
 await open();await page.evaluate(()=>{window.detailImage=document.querySelector('.detail-art img');document.querySelector('.detail-creator .creator').focus();document.querySelector('#game-detail').scrollTop=200;});
 game.reactions.push({...friend,emoji:'🚀',text:'New reaction',updated:now+2});
 await page.evaluate(()=>refreshTestLibrary());assert(await page.evaluate(()=>document.querySelector('.detail-art img')===detailImage));assert(await dialog.locator('.detail-creator .creator').evaluate(e=>e===document.activeElement));assert(await dialog.evaluate(e=>e.scrollTop>=200));await close();
 // Admin actions live only in the full popup; Play menus have no reactions or moderation.
 assert.equal(await page.locator('.game .game-admin-menu,.game .play-menu-items .chips').count(),0);
 assert.equal(await page.locator('.game .play-menu-items').getByRole('button',{name:/Hide game|Unhide game|Make invisible|Make visible/}).count(),0);
 await open();await dialog.getByLabel('Admin actions',{exact:true}).click();
 await page.evaluate(()=>refreshTestLibrary());assert(await dialog.locator('.game-admin-menu').evaluate(e=>e.open),'polling preserves the open admin menu');
 await page.keyboard.press('Escape');assert(await dialog.isVisible());assert.equal(await dialog.locator('.game-admin-menu').evaluate(e=>e.open),false);
 await close();
 // Exercise failure, pending requests, dismissal and retry independently in each layout.
 for(const theme of ['nightglass','cartridge-club']){
  await page.evaluate(t=>InfuserThemes.select(t),theme);
  for(const width of [1280,390,320]){
   await page.setViewportSize({width,height:900});await open();
   const menu=dialog.locator('.game-admin-menu'),trigger=dialog.getByLabel('Admin actions',{exact:true});
   await trigger.click();
   assert(await menu.locator('.game-admin-items').evaluate(e=>{const r=e.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth;}));
   await page.screenshot({path:new URL(`${theme}-${width}-admin.png`,out).pathname});
   await dialog.locator('.detail-authors').click();assert.equal(await menu.evaluate(e=>e.open),false);
   await trigger.click();failVisibility=true;releaseVisibility=undefined;
   const action=menu.getByRole('button',{name:'Hide game',exact:true});
   const requested=page.waitForRequest(r=>r.url().endsWith('/visibility'));
   await action.click();await requested;
   assert(await action.isDisabled());
   const count=writes.filter(w=>w.path.endsWith('/visibility')).length;
   await action.evaluate(e=>e.click());await page.evaluate(()=>refreshTestLibrary());
   assert(await action.isDisabled());assert.equal(writes.filter(w=>w.path.endsWith('/visibility')).length,count);
   assert(releaseVisibility);releaseVisibility();
   await menu.getByRole('alert').waitFor();assert.equal(await menu.getByRole('alert').textContent(),'Infuser is restarting. Try again in a moment.');
   assert(await action.isEnabled());assert(await menu.evaluate(e=>e.open));
   await page.evaluate(()=>refreshTestLibrary());assert(await menu.getByRole('alert').isVisible());
   await page.screenshot({path:new URL(`${theme}-${width}-admin-error.png`,out).pathname});
   failVisibility=false;await action.click();
   await page.waitForFunction(()=>document.querySelector('#game-detail .game-visibility')?.textContent==='Hidden');
   await dialog.getByLabel('Admin actions',{exact:true}).click();
   await dialog.getByRole('button',{name:'Unhide game',exact:true}).click();
   await page.waitForFunction(()=>!document.querySelector('#game-detail .game-visibility'));
   await dialog.getByLabel('Admin actions',{exact:true}).click();await page.keyboard.press('Escape');
   assert(await dialog.isVisible());assert.equal(await dialog.locator('.game-admin-menu').evaluate(e=>e.open),false);
   assert(await dialog.getByLabel('Admin actions',{exact:true}).evaluate(e=>e===document.activeElement));await close();
  }
 }
 // Admin visibility moves a game between the shared shelves.
 await page.setViewportSize({width:1280,height:900});await page.locator('#hidden-games').evaluate(e=>e.open=false);await open();await dialog.getByLabel('Admin actions',{exact:true}).click();await dialog.getByRole('button',{name:'Hide game',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('#visible-games .game').length===2);await close();await page.waitForFunction(()=>document.querySelector('#hidden-games > summary')===document.activeElement);
 await page.locator('#hidden-games').evaluate(e=>e.open=true);await page.locator('#hidden-game-list [data-app="firefly-drift"] .game-open').click();await dialog.getByLabel('Admin actions',{exact:true}).click();await dialog.getByRole('button',{name:'Unhide game',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('#visible-games .game').length===3);await close();
 // Presence portrait owns the profile menu, which opens User settings (theme/profile/install/notifications).
 await page.locator('#account-trigger').click();await page.locator('#open-settings').click();await page.locator('#open-profile').click();await page.locator('input[name="avatar"][value="byte-gecko"]').check();await page.locator('#save-profile').click();await page.waitForFunction(()=>!document.querySelector('#profile-dialog').open);assert.equal(me.avatar,'byte-gecko');
 await page.locator('#add-game').click();await page.waitForFunction(()=>document.querySelector('#agent-prompt').value.includes('TEST-ONLY'));await page.locator('#close-add-game').click();
 await page.emulateMedia({reducedMotion:'reduce'});assert.equal(await card().evaluate(e=>getComputedStyle(e).transitionDuration),'0s');
 await page.emulateMedia({forcedColors:'active'});await open();assert(await dialog.isVisible());await close();await page.emulateMedia({forcedColors:'none'});
 // The same detail UI works read-only in operator inspection.
 me.inspecting=true;me.admin=false;await page.reload();await page.locator('.game-open').first().waitFor();await open();assert(await dialog.locator('.actions > button').isDisabled());assert(await dialog.locator('.creator').isDisabled());assert.equal(await dialog.locator('.reaction-choice:disabled').count(),26);await close();
 await page.locator('#recent-toggle').click();assert.equal(await page.locator('#feed .feed-react-toggle').count(),0,'read-only inspection cannot react to updates');await page.locator('#close-recent').click();
 unauthorized=true;await page.reload();await page.locator('#welcome').waitFor();assert(await page.locator('.google img').isVisible());
 assert.deepEqual(errors,[]);
 console.log('PASS: shared details/cards, both themes at desktop/tablet/320/390, launch/guest/join, pending-art transitions, author search, 28-day attenuated reaction stacks, positive reactions with optional notes and bursts, unread Recently, centered card scaling, tags, activity targets, profile, visibility, text safety, inspection, landing, reduced motion and forced colors.');
} finally {await browser.close();}
