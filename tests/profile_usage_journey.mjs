import {ordinaryThemeCalendar} from './support/theme_calendar.mjs';
// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs INFUSER_CHROMIUM=/path/to/chromium node tests/profile_usage_journey.mjs
import assert from 'node:assert/strict';
import {readFile,mkdir} from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const root=new URL('../',import.meta.url);
const browser=await chromium.launch({headless:true,executablePath:process.env.INFUSER_CHROMIUM});
const me={player_id:'self',name:'Alex',avatar:'circuit-fox',avatar_prompt_seen:false,admin:false,avatars:[{id:'circuit-fox',name:'Circuit Fox'},{id:'moss-knight',name:'Moss Knight'},{id:'orbit-racer',name:'Orbit Racer'}]};
let users=['Drew','Alex','Casey','Blair','Éclair'].map(name=>({name,avatar:'circuit-fox'}));
users.push({name:'Émilie',avatar:'moss-knight'});
let mode='normal',held=[],writes=[],reminderCalls=0;
try{
 const context=await browser.newContext({viewport:{width:1200,height:850},serviceWorkers:'block'});
 await context.route('https://library.test/**',async route=>{
  const request=route.request(),path=new URL(request.url()).pathname;
  if(path==='/api/me')return route.fulfill({json:me});
  if(path==='/api/library')return route.fulfill({json:{apps:[],friends:[],tags:[]}});
  if(path==='/api/chat')return route.fulfill({json:{messages:[]}});
  if(path==='/api/profile/picture-prompt'){
   reminderCalls++;const show=!me.avatar&&!me.avatar_prompt_seen;me.avatar_prompt_seen=true;
   return route.fulfill({json:{show}});
  }
  if(path==='/api/profile'){
   if(request.method()==='PUT'){writes.push(request.postDataJSON());me.avatar=request.postDataJSON().avatar;me.avatar_prompt_seen=true;return route.fulfill({json:{avatar:me.avatar}});}
   if(mode==='hold'){held.push(route);return;}
   if(mode==='fail')return route.fulfill({status:503,json:{error:'Unavailable'}});
   return route.fulfill({json:{users}});
  }
  let file=path==='/library'?'web/index.html':path.startsWith('/assets/')?'web/'+path.slice(8):null;
  const avatar=/^\/assets\/avatars\/([^/]+)\/(\d+)\.webp$/.exec(path);if(avatar)file=`assets/avatars/${avatar[1]}-${avatar[2]}.webp`;
  if(file)try{let body=await readFile(new URL(file,root));
  if(file==='web/themes.js')body=ordinaryThemeCalendar(body.toString());if(file==='web/clubhouse.js')body=body.toString()+'\nwindow.profileReminderSettled=()=>!profileReminderBusy;';return route.fulfill({body,contentType:file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':file.endsWith('.webp')?'image/webp':'text/html'});}catch{}
  return route.fulfill({status:404,body:''});
 });
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));
 const dialog=page.locator('#profile-dialog'),option=id=>dialog.locator('label').filter({has:page.locator(`input[value="${id}"]`)});
 const open=async()=>{await page.locator('#account-trigger').click();await page.locator('#open-settings').click();await page.locator('#open-profile').click();};
 const close=()=>page.locator('#close-profile').click();
 await page.goto('https://library.test/library');
 for(const theme of ['nightglass','cartridge-club'])for(const width of [1200,390,320]){
  await page.setViewportSize({width,height:850});await page.evaluate(t=>InfuserThemes.select(t),theme);await open();
  await option('circuit-fox').locator('.profile-usage').waitFor();
  assert.deepEqual(await option('circuit-fox').locator('.profile-usage-badge').allTextContents(),['A','B','C','+2']);
  assert.equal(await option('circuit-fox').locator('.profile-usage-badge').first().getAttribute('title'),'Alex');
  assert.equal(await option('circuit-fox').locator('.profile-usage-badge').last().getAttribute('title'),'Drew, Éclair');
  assert.equal(await option('moss-knight').locator('.profile-usage-badge').textContent(),'É');
  assert.equal(await option('orbit-racer').locator('.profile-usage-badge').count(),0);
  assert.equal(await option('').locator('.profile-usage-badge').count(),0);
  assert(await dialog.getByRole('radio',{name:'Circuit Fox',exact:true}).isChecked());
  assert.match(await option('circuit-fox').locator('.sr-only').textContent(),/Alex, Blair, Casey, Drew, Éclair/);
  assert(await dialog.evaluate(d=>d.scrollWidth<=d.clientWidth),'badges do not widen the dialog');
  for(const bounds of await dialog.locator('.profile-usage').evaluateAll(nodes=>nodes.map(n=>{const a=n.getBoundingClientRect(),b=n.closest('.profile-tile').getBoundingClientRect();return {fits:a.left>=b.left&&a.right<=b.right&&a.bottom<b.bottom};})))assert(bounds.fits,'badges stay inside their tile');
  if(width===390&&theme==='nightglass'){await mkdir(new URL('.local/profile-usage/',root),{recursive:true});await dialog.screenshot({path:new URL('.local/profile-usage/picker.png',root).pathname});}
  await close();
 }
 // Late usage updates decorate existing choices without stealing selection/focus.
 mode='hold';const read=page.waitForRequest(r=>new URL(r.url()).pathname==='/api/profile');await open();await read;
 await dialog.getByRole('radio',{name:'Orbit Racer',exact:true}).check();
 await page.evaluate(()=>{window.selectedProfile=document.activeElement;});
 await held.shift().fulfill({json:{users}});await option('circuit-fox').locator('.profile-usage').waitFor();
 assert(await dialog.getByRole('radio',{name:'Orbit Racer',exact:true}).isChecked());
 assert(await page.evaluate(()=>selectedProfile===document.activeElement));await close();
 // Closing aborts an old read; reopening loads current usage instead.
 const old=page.waitForRequest(r=>new URL(r.url()).pathname==='/api/profile');await open();await old;await close();
 mode='normal';users=[{name:'Zoe',avatar:'orbit-racer'}];await open();await option('orbit-racer').locator('.profile-usage').waitFor();
 await held.shift().fulfill({json:{users:[{name:'Stale',avatar:'moss-knight'}]}}).catch(()=>{});
 assert.equal(await option('moss-knight').locator('.profile-usage-badge').count(),0);
 await dialog.getByRole('radio',{name:'Orbit Racer',exact:true}).check();await page.locator('#save-profile').click();await dialog.waitFor({state:'hidden'});
 assert.deepEqual(writes,[{avatar:'orbit-racer'}],'an occupied picture remains selectable');
 mode='fail';await open();await page.waitForFunction(()=>document.querySelector('#profile-error').textContent.includes('Could not load'));
 assert(await page.locator('#save-profile').isEnabled());await close();assert.deepEqual(errors,[]);
 // A first visit with initials prompts once, including across fresh browser pages.
 mode='normal';me.avatar=null;me.avatar_prompt_seen=false;reminderCalls=0;
 await page.reload();await dialog.waitFor();assert.equal(reminderCalls,1);
 assert(await dialog.getByRole('radio',{name:'Initials',exact:true}).isChecked());
 await page.keyboard.press('Escape');await dialog.waitFor({state:'hidden'});
 const settled=async()=>page.waitForFunction(()=>window.profileReminderSettled?.());
 await page.reload();await settled();assert(await dialog.isHidden());assert.equal(reminderCalls,1);
 await open();assert(await dialog.isVisible());await close();assert.equal(reminderCalls,1);
 // A different profile gets its own reminder; explicitly saving initials finishes it.
 me.player_id='second';me.avatar_prompt_seen=false;await page.reload();await dialog.waitFor();
 assert.equal(reminderCalls,2);await page.locator('#save-profile').click();await dialog.waitFor({state:'hidden'});
 await page.reload();await settled();assert(await dialog.isHidden());assert.equal(reminderCalls,2);
 // Existing pictures and read-only inspection never prompt.
 me.avatar='circuit-fox';me.avatar_prompt_seen=false;await page.reload();await settled();
 assert(await dialog.isHidden());assert.equal(reminderCalls,2);
 me.avatar=null;me.inspecting=true;await page.reload();await settled();
 assert(await dialog.isHidden());assert.equal(reminderCalls,2);
 // An already-open task keeps focus; the reminder waits until that dialog closes.
 me.inspecting=false;me.avatar_prompt_seen=false;me.player_id='third';
 const deferred=await context.newPage();
 await deferred.addInitScript(()=>document.addEventListener('DOMContentLoaded',()=>document.querySelector('#tags-dialog').showModal(),{once:true}));
 await deferred.goto('https://library.test/library');
 await deferred.waitForFunction(()=>window.profileReminderSettled?.());
 assert(await deferred.locator('#tags-dialog').isVisible());
 assert(await deferred.locator('#profile-dialog').isHidden());assert.equal(reminderCalls,2);
 await deferred.locator('#tags-dialog').evaluate(d=>d.close());
 await deferred.locator('#profile-dialog').waitFor();assert.equal(reminderCalls,3);
 await deferred.locator('#close-profile').click();await deferred.close();
 assert.deepEqual(errors,[]);
 console.log('PASS: one-time per-profile reminder, dismissal and reload, saved initials, existing-avatar/inspection exclusions, deferred dialog focus; usage initials, names, overflow and Unicode; both themes at desktop/390/320px; stable focus/selection during loading; refreshed usage, stale request cancellation, occupied-picture saving and graceful read failure.');
}finally{await browser.close();}
