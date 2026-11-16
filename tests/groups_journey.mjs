// Opt-in browser journey against the real Rust fixture in groups_journey.rs.
// INFUSER_GROUP_BROWSER_NODE=/path/to/node cargo test --test groups_journey -- --nocapture
import assert from 'node:assert/strict';
import http from 'node:http';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'../services/peerjs/node_modules/playwright/index.mjs');
const [base,group]=process.argv.slice(2), errors=[];
let page;
const browser=await chromium.launch({headless:true,...(process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
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
  assert.equal(await page.locator('#operator-groups').isVisible(),false);
  await page.locator('#admin > details > summary').click();
  await page.locator('#admin .admin-panel').getByRole('button',{name:'Groups',exact:true}).click();
  await page.getByText('Host health',{exact:true}).click();
  await page.getByText('Monitoring data is stale. Check the host health timer.',{exact:true}).waitFor();
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
  await page.locator('#admin > details > summary').click();
  assert.equal(await page.locator('#operator-groups').isVisible(),true);
  assert.equal(await page.locator('#invite-form').isVisible(),false);
  assert.equal(await page.locator('#credential-form').isVisible(),false);
  await page.locator('#operator-groups').click();
  await page.getByRole('button',{name:'Close groups',exact:true}).click();
  await page.waitForFunction(()=>document.activeElement===document.querySelector('#admin > details > summary'));
  assert.equal(await page.locator('#admin > details').getAttribute('open'),null);
  // Narrow view must keep the return control usable, without horizontal overflow.
  await page.setViewportSize({width:390,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await page.getByRole('button',{name:'Return home'}).click();
  await page.locator('#games .game').waitFor();
  assert.equal(await page.getByRole('button',{name:'Play',exact:true}).isDisabled(),false);
  assert.equal(await page.locator('#group-inspection').count(),0);
  assert.equal(await page.locator('#operator-groups').isVisible(),false);
  await page.locator('#admin > details > summary').click();
  await page.locator('#admin .admin-panel').getByRole('button',{name:'Groups',exact:true}).click();
  await page.getByText('Add a group',{exact:true}).click();
  await page.getByLabel('Group name',{exact:true}).fill('Browser family');
  await page.getByLabel('Admin’s Google email',{exact:true}).fill('browser-admin@example.com');
  await page.getByLabel('Admin’s display name',{exact:true}).fill('Browser admin');
  await page.getByRole('button',{name:'Create group',exact:true}).click();
  await page.locator('.group-row').filter({hasText:'Browser family'}).waitFor();
  await page.getByRole('button',{name:'Close groups',exact:true}).click();
  await page.evaluate(g=>sessionStorage.setItem('infuser.group-inspection',JSON.stringify({group:g,expires:Date.now()-1})),group);
  await page.reload();await page.locator('#games .game').waitFor();
  assert.equal(await page.locator('#group-inspection').count(),0);
  // A family admin without the operator role has ordinary admin tools, no Groups.
  await context.clearCookies();await context.addCookies([{name:'member',value:'admin',url:'https://games.example.com'}]);
  await page.reload();await page.locator('#admin').waitFor();
  assert.equal(await page.locator('#operator-groups').count(),0);
  await page.locator('#admin > details > summary').click();
  assert.equal(await page.locator('#invite-form').isVisible(),true);
  // Ordinary players have neither admin navigation nor the operator entry.
  await context.clearCookies();await context.addCookies([{name:'member',value:'user',url:'https://games.example.com'}]);
  await page.reload();await page.locator('#clubhouse').waitFor();
  assert.equal(await page.locator('#admin').isVisible(),false);
  assert.equal(await page.locator('#operator-groups').count(),0);
  assert.deepEqual(errors,[]);
  console.log('Browser: inspect, cover, read-only chat, return home, mobile, create group, expiry, operator-only admin menu and focus return passed.');
} catch(error){console.error('Page errors:',errors,'Login:',await page?.locator('body').innerText(),'URL:',page?.url());throw error;} finally {await browser.close();}
