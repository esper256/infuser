import assert from 'node:assert/strict';
import {readFile,mkdir} from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.INFUSER_CHROMIUM});
try {
 const page=await browser.newPage({viewport:{width:390,height:844}});
 await page.route('https://updates.test/**',async route=>{
  const path=new URL(route.request().url()).pathname;
  if(path==='/')return route.fulfill({contentType:'text/html',body:'<link rel="stylesheet" href="/themes/nightglass.css"><link rel="stylesheet" href="/clubhouse.css"><div class="account-menu" id="profile"></div><dialog id="admin" class="admin"><div class="panel-heading"><h2>Admin tools</h2></div><div class="admin-panel"></div></dialog>'});
  const body=await readFile(new URL('../web'+path,import.meta.url),'utf8');
  await route.fulfill({contentType:path.endsWith('.js')?'text/javascript':'text/css',body});
 });
 await page.goto('https://updates.test/');
 await page.evaluate(async()=>{
  const {initServerUpdates,humanWait}=await import('/server-updates.js');
  window.humanWait=humanWait;
  window.state={configured:true,current_build:'a'.repeat(40),current_built_at:Date.now()/1000-172800,release_built_at:Date.now()/1000-7200,publishing:true,available:true,available_since:Date.now()/1000-3720,checked_at:Date.now()/1000,report:{release:'b'.repeat(40),phase:'waiting'},activity:[
    {holds:true,kind:'publishing',summary:'Receiving “Rocket Game” from an agent, started 3 min ago.',effect:'Updating now loses this publish. The agent has to send the game again.'},
    {holds:false,kind:'playing',summary:'Alex has “Pond” open in the browser.',effect:'The game keeps running. The open tab checks in regularly, and that check-in does not hold the update.'}
  ]};
  window.posts=[];
  const api=async(path,options={})=>{
   if(window.offline)throw Error('offline');
   if(options.method==='POST'){
    window.posts.push(JSON.parse(options.body));
    await new Promise(r=>setTimeout(r,50));
    window.state.authorized_until=Date.now()/1000+600;
   }
   return structuredClone(window.state);
  };
  const panel=document.querySelector('.admin-panel');
  initServerUpdates({panel,api,me:{operator:false,admin:true}});
  if(document.querySelector('#server-update'))throw Error('Group admin got server controls');
  window.refresh=initServerUpdates({panel,api,me:{operator:true}});
  document.querySelector('#admin').showModal();
 });
 await page.getByRole('heading',{name:'Server updates'}).waitFor();
 assert.equal(await page.locator('#profile #server-update').count(),0);
 assert.equal(await page.locator('#admin #server-update').count(),1);
 await page.getByText('Update waiting 1 h 2 min',{exact:true}).waitFor();
 await page.getByText('Receiving “Rocket Game” from an agent, started 3 min ago.',{exact:true}).waitFor();
 await page.getByText('Alex has “Pond” open in the browser.',{exact:true}).waitFor();
 await page.getByText('The update is waiting.',{exact:true}).waitFor();
 assert.equal(await page.locator('.update-build code').allTextContents().then(v=>v.join(',')),'aaaaaaaaaaaa,bbbbbbbbbbbb');
 assert.match(await page.locator('.update-build-age').first().textContent(),/^Built 2 d 0 h ago$/);
 assert.match(await page.locator('.update-build-age').last().textContent(),/^Built 2 h 0 min ago$/);
 assert(await page.locator('.update-build time').first().getAttribute('datetime'));
 assert(Number(await page.locator('.update-publishing strong').evaluate(e=>getComputedStyle(e).fontWeight))>=700);
 for(const phase of ['checking','downloading','waiting','failed','activating']){
  await page.evaluate(async phase=>{state.report.phase=phase;await refresh();},phase);
  assert(await page.locator('.update-publishing').isVisible(),phase+' must not hide publishing');
  assert(await page.getByText('Alex has “Pond” open in the browser.',{exact:true}).isVisible(),phase);
 }
 await page.evaluate(async()=>{state.report.phase='waiting';await refresh();});
 // Stable data must not churn the panel during polling.
 assert.equal(await page.evaluate(async()=>{let count=0;const observer=new MutationObserver(records=>count+=records.length);observer.observe(document.querySelector('#server-update'),{subtree:true,childList:true,characterData:true,attributes:true});await refresh();await Promise.resolve();observer.disconnect();return count;}),0);
 for(const theme of ['nightglass','cartridge-club']){
  await page.evaluate(theme=>{document.querySelector('link').href='/themes/'+theme+'.css';document.documentElement.dataset.theme=theme;},theme);
  for(const width of [320,390,1280]){
   await page.setViewportSize({width,height:1000});
   await mkdir('.local/operator-update',{recursive:true});
   await page.screenshot({path:'.local/operator-update/'+theme+'-'+width+'.png'});
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
   assert(await page.locator('.update-builds').evaluate(e=>e.scrollWidth<=e.clientWidth));
  }
 }
 await page.setViewportSize({width:390,height:844});
 await page.getByRole('button',{name:'Update anyway',exact:true}).click();
 const confirm=page.locator('.update-confirm');
 await confirm.getByText('loses this publish').waitFor();
 await confirm.getByText('does not hold the update').waitFor();
 await page.getByRole('button',{name:'Cancel',exact:true}).click();
 assert.equal(await page.evaluate(()=>posts.length),0);
 await page.getByRole('button',{name:'Update anyway',exact:true}).click();
 await mkdir('.local/operator-update',{recursive:true});
 await page.screenshot({path:'.local/operator-update/admin-mobile.png'});
 assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
 // New releases may arrive while the confirmation is open; approval is for
 // one activation from this running build, not a stale candidate.
 await page.evaluate(()=>{state.report.release='c'.repeat(40);});
 await page.getByRole('button',{name:'Authorize one update',exact:true}).dblclick();
 await page.getByRole('button',{name:'Update authorized',exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>posts.length),1);
 assert.equal(await page.evaluate(()=>posts[0].current_build),'a'.repeat(40));
 assert.match(await page.evaluate(()=>posts[0].request_id),/^[a-f0-9]{32}$/);
 await page.evaluate(async()=>{state.authorized_until=null;offline=true;await refresh();});
 assert(await page.locator('#server-update > button').isDisabled());
 assert(await page.locator('.update-publishing').isVisible(),'offline state retains the last publishing warning');
 await page.evaluate(async()=>{offline=false;state.checked_at=Date.now()/1000-181;await refresh();});
 assert(await page.getByRole('button',{name:'Update anyway',exact:true}).isDisabled());
 assert(!await page.locator('.update-message').textContent(),'successful refresh clears the connection warning');
 await page.evaluate(async()=>{state.checked_at=Date.now()/1000;state.report.phase='failed';await refresh();});
 assert(await page.getByRole('button',{name:'Update anyway',exact:true}).isDisabled());
 await page.evaluate(async()=>{state.available=false;state.report.phase='current';await refresh();});
 assert(await page.locator('#server-update').isVisible(),'running build remains visible when up to date');
 assert(await page.getByText('Already on latest',{exact:true}).isVisible());
 assert(await page.getByRole('button',{name:'Update anyway',exact:true}).isHidden());
 await page.evaluate(async()=>{state.current_built_at=null;state.release_built_at=null;state.available=true;state.report.phase='waiting';state.publishing=false;state.activity=[{holds:false,kind:'playing',summary:'Alex has “Pond” open in the browser.',effect:'The game keeps running. The open tab checks in regularly, and that check-in does not hold the update.'}];await refresh();});
 assert(await page.locator('.update-publishing').isHidden());
 assert(await page.getByText('The update will install on the next check, usually within a minute.',{exact:true}).isVisible());
 assert.equal(await page.getByText('Build time unknown',{exact:true}).count(),2);
 await page.evaluate(async()=>{state.configured=false;state.available=false;state.checked_at=0;await refresh();});
 assert(await page.getByText('Not configured',{exact:true}).isVisible());
 assert(await page.locator('#server-update').isVisible());
 assert.equal(await page.evaluate(()=>humanWait(100,100+90060)),'1 d 1 h');
 console.log('Passed: build IDs/ages/dates, publishing priority across updater states, both themes at 320/390/1280, zero unchanged-refresh mutations, operator-only control, confirmation, single POST, supersession, stale/offline/failure and legacy metadata states.');
} finally {await browser.close();}
