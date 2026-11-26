// Local mocked API, real assets and production CSP; no live household or host.
import assert from 'node:assert/strict';
import {readFile,mkdir} from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const root=new URL('../',import.meta.url),out=new URL('.local/thanksgiving/',root);
await mkdir(out,{recursive:true});
const browser=await chromium.launch({headless:true,executablePath:process.env.INFUSER_CHROMIUM});
const original={theme:'nightglass',schemes:{nightglass:'custom'},custom:{nightglass:{primary:'#123456',accent:'#fedcba',secondary:'#89abcd'}}};
const me={player_id:'self',name:'Alex',avatar:'',avatar_prompt_seen:true,admin:false,profile_count:1,group:{id:'home',name:'Home'},appearance:original,avatars:[]};
const app={id:'lantern-keep',title:'Lantern Keep',description:'A cozy adventure.',tags:['cozy'],updated:1795716000,author:{id:'self',name:'Alex'},reactions:[],players:[],join_targets:[]};
const csp="default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'";
const appearanceWrites=[];
try{
 const context=await browser.newContext({viewport:{width:1280,height:900},timezoneId:'America/Los_Angeles',serviceWorkers:'block'});
 await context.route('https://library.test/**',async route=>{
   const path=new URL(route.request().url()).pathname;
   if(path==='/api/me')return route.fulfill({json:me});
   if(path==='/api/profile/appearance'){
     if(route.request().method()==='PUT')appearanceWrites.push(route.request().postDataJSON());
     return route.fulfill({json:{appearance:me.appearance}});
   }
   if(path==='/api/library')return route.fulfill({json:{apps:[app],friends:[{id:'self',name:'Alex',status:'active'}],tags:[{id:'cozy',label:'Cozy',count:1}]}});
   if(path==='/api/chat')return route.fulfill({json:{messages:[]}});
   if(path==='/api/voice')return route.fulfill({json:{configured:false,available:false,participants:[],self:null,settings:{}}});
   if(path==='/api/visits')return route.fulfill({json:{enabled:false}});
   if(path==='/api/client-state')return route.fulfill({json:{version:'test',game_open:false}});
   if(path.startsWith('/api/'))return route.fulfill({json:{}});
   let file=path==='/library'?'web/index.html':path.startsWith('/assets/')?'web/'+path.slice(8):null;
   if(file){
     try{
       let body=await readFile(new URL(file,root));
       if(file==='web/clubhouse.js')body=body.toString()+'\nwindow.testHolidayConfetti=updateConfetti;';
       const type=file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':file.endsWith('.png')?'image/png':file.endsWith('.svg')?'image/svg+xml':file.endsWith('.woff2')?'font/woff2':'text/html';
       return route.fulfill({body,contentType:type,headers:{'Content-Security-Policy':csp}});
     }catch{}
   }
   return route.fulfill({status:404,body:''});
 });
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.addInitScript(()=>{window.cspViolations=[];document.addEventListener('securitypolicyviolation',event=>cspViolations.push(event.violatedDirective));});
 await page.clock.setFixedTime(new Date('2026-11-26T12:00:00-08:00'));
 await page.goto('https://library.test/library');await page.locator('#account-trigger').waitFor();await page.locator('.game-open').waitFor();
 assert.equal(await page.evaluate(()=>InfuserThemes.current),'thanksgiving');
 assert.deepEqual(await page.evaluate(()=>InfuserThemes.snapshot()),original);
 assert.equal(await page.locator('.brand-wordmark').getAttribute('aria-label'),'Thankfuser Library');
 await page.locator('.brand-icon').evaluate(img=>img.decode());
 assert((await page.locator('.brand-icon').getAttribute('src')).endsWith('/thanksgiving/turkey.png'));
 assert.equal(await page.locator('.thankfuser-wordmark').innerText(),'Thankfuser');
 await page.locator('#recent-toggle').click();assert(await page.locator('#activity').isVisible());await page.locator('#close-recent').click();
 const saved=await page.evaluate(()=>localStorage.getItem('infuser.appearance.self'));
 await page.locator('#account-trigger').click();await page.locator('#open-settings').click();
 assert(await page.locator('#theme-scheme-group').isHidden(),'holiday cannot edit saved color schemes');
 assert(await page.locator('#theme-picker').isDisabled());assert.equal(await page.locator('#theme-picker').inputValue(),'thanksgiving');
 assert(await page.getByText('Happy Thanksgiving! Your usual theme and colors return tomorrow.').isVisible());
 await page.evaluate(async()=>{await InfuserThemes.select('castle-keep');InfuserThemes.setCustomColors({primary:'#ffffff'});InfuserThemes.resetCustomColors();});
 assert.equal(await page.evaluate(()=>InfuserThemes.current),'thanksgiving');
 assert.deepEqual(await page.evaluate(()=>InfuserThemes.snapshot()),original);
 await page.locator('#close-settings').click();
 for(const width of [1280,390,320]){
   await page.setViewportSize({width,height:900});
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no holiday horizontal overflow at '+width);
   await page.screenshot({path:new URL('library-'+width+'.png',out).pathname});
   await page.evaluate(()=>testHolidayConfetti(document.querySelector('.game-open')));
   const flock=page.locator('.turkey-stampede');await flock.waitFor();
   assert.equal(await page.locator('.update-confetti').count(),0);
   assert.equal(await flock.getAttribute('aria-hidden'),'true');
   assert.equal(await flock.evaluate(el=>getComputedStyle(el).pointerEvents),'none');
   assert.equal(await flock.locator('.stampede-turkey').count(),width<600?12:20);
   // Await an actual animation phase, not a sleep, to inspect the stampede mid-flight.
   await page.waitForFunction(()=>document.querySelector('.stampede-turkey')?.getAnimations()[0]?.currentTime>650);
   await page.screenshot({path:new URL('flock-'+width+'.png',out).pathname});
   await flock.waitFor({state:'detached'});
   assert.equal(await page.locator('.reaction-burst').count(),0);
 }
 await page.emulateMedia({reducedMotion:'reduce'});
 await page.evaluate(()=>testHolidayConfetti(document.querySelector('.game-open')));
 assert.equal(await page.locator('.reaction-burst').count(),0,'reduced motion suppresses the flock');
 await page.emulateMedia({reducedMotion:'no-preference'});
 // Repeated bursts replace one another rather than accumulating flocks.
 await page.evaluate(()=>{for(let i=0;i<3;i++)testHolidayConfetti(document.querySelector('.game-open'));});
 assert.equal(await page.locator('.turkey-stampede').count(),1);
 await page.locator('.turkey-stampede').waitFor({state:'detached'});
 assert.equal(await page.evaluate(()=>localStorage.getItem('infuser.appearance.self')),saved);
 assert.deepEqual(appearanceWrites,[],'holiday never saves an appearance');
 await page.clock.setFixedTime(new Date('2026-11-27T00:00:01-08:00'));await page.evaluate(()=>InfuserThemes.refreshSeason());
 assert.equal(await page.evaluate(()=>InfuserThemes.current),'nightglass');
 assert.equal(await page.evaluate(()=>InfuserThemes.currentScheme),'custom');
 assert.deepEqual(await page.evaluate(()=>InfuserThemes.snapshot()),original);
 assert.equal(await page.evaluate(()=>InfuserThemes.catalog.some(t=>t.id==='thanksgiving')),false);
 assert((await page.locator('.brand-icon').getAttribute('src')).startsWith('data:'));
 await page.locator('#account-trigger').click();await page.locator('#open-settings').click();
 assert(await page.locator('#theme-picker').isEnabled());assert.equal(await page.locator('#theme-picker option[value=thanksgiving]').count(),0);await page.locator('#close-settings').click();
 await page.evaluate(()=>testHolidayConfetti(document.querySelector('.game-open')));
 await page.locator('.update-confetti').waitFor();assert.equal(await page.locator('.turkey-stampede').count(),0);
 await page.locator('.update-confetti').waitFor({state:'detached'});
 assert.deepEqual(await page.evaluate(()=>cspViolations),[]);
 // Reload outside the holiday cannot preserve the temporary choice.
 await page.reload();await page.locator('#account-trigger').waitFor();assert.equal(await page.evaluate(()=>InfuserThemes.current),'nightglass');
 assert.deepEqual(await page.evaluate(()=>InfuserThemes.snapshot()),original);
 assert.deepEqual(await page.evaluate(()=>cspViolations),[]);assert.deepEqual(errors,[]);
 // The production midnight timeout enters and exits without a reload or manual refresh.
 const rollover=await context.newPage();
 await rollover.clock.install({time:new Date('2026-11-25T23:59:59-08:00')});
 await rollover.goto('https://library.test/library');await rollover.locator('#account-trigger').waitFor();
 assert.equal(await rollover.evaluate(()=>InfuserThemes.current),'nightglass');
 await rollover.clock.fastForward(1200);
 await rollover.waitForFunction(()=>InfuserThemes.current==='thanksgiving');
 await rollover.clock.fastForward(86400000);
 await rollover.waitForFunction(()=>InfuserThemes.current==='nightglass');
 assert.deepEqual(await rollover.evaluate(()=>InfuserThemes.snapshot()),original);
 assert.deepEqual(appearanceWrites,[]);
 await context.close();
 console.log('PASS: Thanksgiving brand, forced theme, saved custom appearance, all widths, Recently, flock cleanup/reduced motion, next-day restoration and production CSP.');
}finally{await browser.close();}
