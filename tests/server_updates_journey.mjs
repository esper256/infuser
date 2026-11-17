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
  window.state={configured:true,current_build:'old',available:true,available_since:Date.now()/1000-3720,checked_at:Date.now()/1000,report:{release:'b'.repeat(40),phase:'waiting'},reasons:['Waiting for games to finish','Waiting for game uploads or publishing to finish']};
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
 await page.getByRole('heading',{name:'Update available'}).waitFor();
 assert.equal(await page.locator('#profile #server-update').count(),0);
 assert.equal(await page.locator('#admin #server-update').count(),1);
 await page.getByText('Waiting 1 h 2 min',{exact:true}).waitFor();
 await page.getByText(/Waiting for game uploads or publishing/).waitFor();
 await page.getByRole('button',{name:'Update anyway',exact:true}).click();
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
 assert.equal(await page.evaluate(()=>posts[0].current_build),'old');
 assert.match(await page.evaluate(()=>posts[0].request_id),/^[a-f0-9]{32}$/);
 await page.evaluate(async()=>{state.authorized_until=null;offline=true;await refresh();});
 assert(await page.locator('#server-update > button').isDisabled());
 await page.evaluate(async()=>{offline=false;state.checked_at=Date.now()/1000-181;await refresh();});
 assert(await page.getByRole('button',{name:'Update anyway',exact:true}).isDisabled());
 await page.evaluate(async()=>{state.checked_at=Date.now()/1000;state.report.phase='failed';await refresh();});
 assert(await page.getByRole('button',{name:'Update anyway',exact:true}).isDisabled());
 await page.evaluate(async()=>{state.available=false;await refresh();});
 assert(await page.locator('#server-update').isHidden());
 assert.equal(await page.evaluate(()=>humanWait(100,100+90060)),'1 d 1 h');
 console.log('Passed: Admin tools placement, operator-only control, wait/reasons, confirmation, single POST, supersession and stale/offline/failure states.');
} finally {await browser.close();}
