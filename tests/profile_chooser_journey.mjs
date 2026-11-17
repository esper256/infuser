import assert from 'node:assert/strict';
import http from 'node:http';
import {mkdir} from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'../services/peerjs/node_modules/playwright/index.mjs');
const [base,home,away]=process.argv.slice(2),errors=[];
const browser=await chromium.launch({headless:true,...(process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
async function context(who){
  const c=await browser.newContext();await c.addCookies([{name:'member',value:who,url:'https://games.example.com'}]);
  c.on('page',p=>{p.setDefaultTimeout(10000);p.on('pageerror',e=>errors.push(e.message));});
  await c.route('https://games.example.com/**',async route=>{
    const request=route.request(),url=new URL(request.url()),headers={...await request.allHeaders(),host:'games.example.com'};
    delete headers['content-length'];delete headers['accept-encoding'];
    const response=await new Promise((resolve,reject)=>{
      const proxy=http.request(base+url.pathname+url.search,{method:request.method(),headers},r=>{
        const chunks=[];r.on('data',b=>chunks.push(b));r.on('end',()=>resolve({status:r.statusCode,headers:r.headers,body:Buffer.concat(chunks)}));
      });proxy.on('error',reject);proxy.end(request.postDataBuffer()||undefined);
    });await route.fulfill(response);
  });return c;
}
const choose=async(page,name)=>{await page.getByRole('button',{name:new RegExp(name)}).click();await page.waitForURL('**/library');await page.locator('#account-trigger').waitFor();};
try{
  const single=await context('other'),direct=await single.newPage(),urls=[];
  direct.on('framenavigated',f=>{if(f===direct.mainFrame())urls.push(f.url());});
  await direct.goto('https://games.example.com/library');await direct.locator('#account-trigger').waitFor();
  assert(!urls.some(url=>url.includes('/profiles')));assert.equal(await direct.locator('#switch-profile').count(),0);
  const multi=await context('one'),first=await multi.newPage();
  await first.goto('https://games.example.com/library');await first.waitForURL('**/profiles?*');
  assert.equal(await first.locator('.profile-choice').count(),2);
  await first.locator('.profile-choice img').evaluate(img=>img.decode());
  await mkdir('.local/profile-chooser',{recursive:true});await first.screenshot({path:'.local/profile-chooser/desktop.png'});
  await first.setViewportSize({width:390,height:844});
  assert(await first.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await first.screenshot({path:'.local/profile-chooser/mobile.png'});
  await choose(first,'Original profile');
  assert.equal(await first.evaluate(()=>sessionStorage.getItem('infuser.profile')),home);
  assert.equal(await first.locator('#games .game').count(),1);
  await first.locator('#games .game img').first().evaluate(img=>img.decode());
  const second=await multi.newPage();await second.goto('https://games.example.com/library');await second.waitForURL('**/profiles?*');
  await choose(second,'Away Profile');
  assert.equal(await second.evaluate(()=>sessionStorage.getItem('infuser.profile')),away);
  await second.locator('#games .game img').first().evaluate(img=>img.decode());
  // A different tab stays pinned while this one switches.
  await second.evaluate(()=>{sessionStorage.setItem('infuser.visit','old-visit');sessionStorage.setItem('infuser.group-inspection','old-inspection');});
  await second.locator('#account-trigger').click();await second.locator('#switch-profile').click();await second.waitForURL('**/profiles?*');
  assert.equal(await second.evaluate(()=>sessionStorage.getItem('infuser.visit')),null);
  assert.equal(await second.evaluate(()=>sessionStorage.getItem('infuser.group-inspection')),null);
  assert.equal(await first.evaluate(()=>sessionStorage.getItem('infuser.profile')),home);
  await choose(second,'Away Profile');
  await first.locator('#chat-toggle').click();await first.locator('#chat-text').fill('Still my original group');await first.locator('#chat-text').press('Enter');
  await first.getByText('Still my original group',{exact:true}).waitFor();
  await second.locator('#chat-toggle').click();await second.getByText('Only away-game',{exact:true}).waitFor();
  assert.equal(await second.getByText('Still my original group',{exact:true}).count(),0);
  // Notification clicks resolve the subscription owner, not whichever tab was last used.
  await second.evaluate(id=>localStorage.setItem('infuser-push-owner',id),home);
  const notification=await multi.newPage();await notification.goto('https://games.example.com/profiles?notification=chat&next='+encodeURIComponent('/library?chat#chat'));
  await notification.waitForURL('**/library?chat#chat');await notification.locator('#account-trigger').waitFor();
  assert.equal(await notification.evaluate(()=>sessionStorage.getItem('infuser.profile')),home);
  await notification.getByText('Still my original group',{exact:true}).waitFor();
  const pair=await second.evaluate(async()=>{const r=await fetch('/api/native/v1/device/start',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({challenge:'a'.repeat(64)})});return r.json();});
  const desktop=await multi.newPage();await desktop.goto(pair.verification_uri);await desktop.waitForURL('**/profiles?*');
  await desktop.getByRole('button',{name:/Away Profile/}).click();await desktop.waitForURL('**/desktop?pair=*');
  await desktop.getByText('Connect as Away Profile in Away group?',{exact:true}).waitFor();
  await desktop.getByRole('button',{name:'Connect Infuser Play',exact:true}).click();await desktop.getByText('Connected! Infuser Play will open your game.',{exact:true}).waitFor();
  assert.deepEqual(errors,[]);
  console.log('Browser: single-profile bypass, chooser, mobile, avatars/artwork, switching, independent tabs/chat and native login handoff passed.');
}finally{await browser.close();}
