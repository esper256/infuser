// Opt-in browser journey against the real Rust fixture in groups_journey.rs.
// INFUSER_GROUP_BROWSER_NODE=/path/to/node cargo test --test groups_journey -- --nocapture
import assert from 'node:assert/strict';
import http from 'node:http';
import {chromium} from '../services/peerjs/node_modules/playwright/index.mjs';
const [base,group]=process.argv.slice(2), errors=[];
let page;
const browser=await chromium.launch({headless:true});
try {
  const context=await browser.newContext();
  await context.addCookies([{name:'member',value:'frizz',url:'https://games.example.com'}]);
  await context.route('https://games.example.com/**',async route=>{
    const request=route.request(),url=new URL(request.url());
    const headers={...await request.allHeaders(),host:'games.example.com'};
    delete headers['content-length'];delete headers['accept-encoding'];
    const response=await new Promise((resolve,reject)=>{
      const proxy=http.request(base+url.pathname+url.search,{method:request.method(),headers},r=>{
        const chunks=[];r.on('data',b=>chunks.push(b));r.on('end',()=>resolve({status:r.statusCode,headers:r.headers,body:Buffer.concat(chunks)}));
      });proxy.on('error',reject);proxy.end(request.postDataBuffer()||undefined);
    });
    await route.fulfill(response);
  });
  page=await context.newPage();page.setDefaultTimeout(10000);page.on('console',m=>{if(m.type()==='error')console.error('Browser console:',m.text());});page.on('pageerror',error=>errors.push(error.message));
  await page.goto('https://games.example.com/library');
  await page.locator('#games .game').waitFor();
  assert.equal(await page.locator('#games .game').count(),1);
  await page.getByRole('button',{name:'Groups',exact:true}).click();
  const family=page.locator('.group-row').filter({hasText:'Other family'});
  await family.getByRole('button',{name:'View',exact:true}).click();
  await page.locator('#group-inspection').waitFor();
  assert.match(await page.locator('#group-inspection').innerText(),/Other family.*Read only/);
  await page.locator('#games .game').waitFor();
  assert.equal(await page.getByRole('button',{name:'Play',exact:true}).isDisabled(),true);
  assert.equal(await page.locator('#add-game').isVisible(),false);
  assert.equal(await page.locator('#friends').innerText().then(v=>v.includes('Frizz')),false);
  await page.waitForFunction(()=>document.querySelector('#games img')?.naturalWidth===1024);
  await page.locator('#chat-toggle').click();
  await page.getByText('Other family only',{exact:true}).waitFor();
  assert.equal(await page.locator('#chat-form').isVisible(),false);
  await page.locator('#chat-close').click();
  // Narrow view must keep the return control usable, without horizontal overflow.
  await page.setViewportSize({width:390,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await page.getByRole('button',{name:'Return home'}).click();
  await page.locator('#games .game').waitFor();
  assert.equal(await page.getByRole('button',{name:'Play',exact:true}).isDisabled(),false);
  assert.equal(await page.locator('#group-inspection').count(),0);
  await page.getByRole('button',{name:'Groups',exact:true}).click();
  await page.getByText('Add a group',{exact:true}).click();
  await page.getByLabel('Group name',{exact:true}).fill('Browser family');
  await page.getByLabel('Adult’s Google email',{exact:true}).fill('browser-adult@example.com');
  await page.getByLabel('Adult’s display name',{exact:true}).fill('Browser adult');
  await page.getByRole('button',{name:'Create group',exact:true}).click();
  await page.locator('.group-row').filter({hasText:'Browser family'}).waitFor();
  await page.getByRole('button',{name:'Close groups',exact:true}).click();
  await page.evaluate(g=>sessionStorage.setItem('infuser.group-inspection',JSON.stringify({group:g,expires:Date.now()-1})),group);
  await page.reload();await page.locator('#games .game').waitFor();
  assert.equal(await page.locator('#group-inspection').count(),0);
  assert.deepEqual(errors,[]);
  console.log('Browser: inspect, cover, read-only chat, return home, mobile, create group, expiry passed.');
} catch(error){console.error('Page errors:',errors,'Login:',await page?.locator('body').innerText(),'URL:',page?.url());throw error;} finally {await browser.close();}
