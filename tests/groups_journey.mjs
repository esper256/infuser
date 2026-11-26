import {ordinaryThemeCalendar} from './support/theme_calendar.mjs';
// Opt-in browser journey against the real Rust fixture in groups_journey.rs.
// INFUSER_GROUP_BROWSER_NODE=/path/to/node cargo test --test groups_journey -- --nocapture
import assert from 'node:assert/strict';
import http from 'node:http';
import {mkdir} from 'node:fs/promises';
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
    if(url.pathname==='/assets/themes.js'){response.body=ordinaryThemeCalendar(response.body.toString());delete response.headers['content-length'];}
    await route.fulfill(response);
  });
  page=await context.newPage();page.setDefaultTimeout(10000);page.on('console',m=>{if(m.type()==='error')console.error('Browser console:',m.text());});page.on('pageerror',error=>errors.push(error.message));
  // Fresh fixture profiles may show the optional avatar chooser on sign-in.
  await page.addLocatorHandler(page.locator('#profile-dialog[open]'),()=>page.locator('#close-profile').click());
  await page.goto('https://games.example.com/library');
  await page.locator('#games .game').waitFor();
  assert.equal(await page.locator('#games .game').count(),1);
  assert.equal(await page.locator('#operator-groups').isVisible(),false);
  await page.locator('#account-trigger').click();await page.locator('#open-admin').click();
  await page.locator('#admin .admin-panel').getByRole('button',{name:'Groups',exact:true}).click();
  const homeRow=page.locator('.group-row').filter({has:page.getByRole('button',{name:'Home',exact:true})});
  await homeRow.getByRole('button',{name:'Rename'}).click();
  const homeName=page.getByRole('textbox',{name:'New name for Family Arcade'});
  await homeName.fill('Arcade Club');await homeName.press('Enter');
  await homeRow.getByText('Arcade Club',{exact:true}).waitFor();
  await homeRow.getByRole('button',{name:'Rename'}).click();
  await page.getByRole('textbox',{name:'New name for Arcade Club'}).press('Escape');
  await homeRow.getByText('Arcade Club',{exact:true}).waitFor();
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
  await page.locator('#chat-log').press('Escape');
  await page.locator('#account-trigger').click();await page.locator('#open-admin').click();
  assert.equal(await page.locator('#operator-groups').isVisible(),true);
  assert.equal(await page.locator('#invite-form').isVisible(),false);
  assert.equal(await page.locator('#credential-form').isVisible(),false);
  await page.locator('#operator-groups').click();
  await page.locator('.group-row').first().waitFor();
  assert.equal(await page.locator('#groups-dialog').getByRole('button',{name:'Rename'}).count(),0);
  await page.getByRole('button',{name:'Close groups',exact:true}).click();
  await page.waitForFunction(()=>document.activeElement===document.querySelector('#operator-groups'));
  assert(await page.locator('#admin').isVisible());
  await page.locator('#close-admin').click();
  await page.waitForFunction(()=>document.activeElement===document.querySelector('#account-trigger'));
  // Narrow view must keep the return control usable, without horizontal overflow.
  await page.setViewportSize({width:390,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await page.getByRole('button',{name:'Return home'}).click();
  await page.locator('#games .game').waitFor();
  assert.equal(await page.getByRole('button',{name:'Play',exact:true}).isDisabled(),false);
  assert.equal(await page.locator('#group-inspection').count(),0);
  assert.equal(await page.locator('#operator-groups').isVisible(),false);
  await page.locator('#account-trigger').click();await page.locator('#open-admin').click();
  await page.locator('#admin .admin-panel').getByRole('button',{name:'Groups',exact:true}).click();
  await homeRow.getByText('Arcade Club',{exact:true}).waitFor();
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
  await page.reload();await page.locator('#open-admin').waitFor({state:'attached'});
  assert.equal(await page.locator('#operator-groups').count(),0);
  await page.locator('#account-trigger').click();await page.locator('#open-admin').click();
  await page.locator('#admin summary').filter({hasText:'Invite a player'}).click();
  await page.locator('#invite-form').waitFor();
  // The modal stays nearly full-screen, with an always-reachable close button.
  for(const theme of ['nightglass','cartridge-club'])for(const width of [1280,390,320]){
    await page.setViewportSize({width,height:850});await page.evaluate(t=>InfuserThemes.select(t),theme);
    const dialog=page.locator('#admin'),bounds=await dialog.boundingBox();
    assert(bounds.width>=width-50&&bounds.height>=800,'admin tools use almost the full viewport');
    assert(bounds.x>=0&&bounds.y>=0&&bounds.x+bounds.width<=width&&bounds.y+bounds.height<=850);
    assert(await dialog.evaluate(d=>d.scrollWidth<=d.clientWidth),'dialog has no horizontal overflow');
    const before=await page.locator('#close-admin').boundingBox();
    await page.locator('.admin-panel').evaluate(e=>e.scrollTop=e.scrollHeight);
    assert.deepEqual(await page.locator('#close-admin').boundingBox(),before,'close control stays fixed while tools scroll');
    await page.locator('.admin-panel').evaluate(e=>e.scrollTop=0);
    await mkdir(new URL('../.local/admin-dialog/',import.meta.url),{recursive:true});
    await page.screenshot({path:new URL(`../.local/admin-dialog/${theme}-${width}.png`,import.meta.url).pathname});
  }
  await page.locator('#invite-form input[name="name"]').fill('Unfinished invitation');
  await page.keyboard.press('Escape');await page.locator('#admin').waitFor({state:'hidden'});
  assert(await page.locator('#account-trigger').evaluate(e=>e===document.activeElement));
  await page.locator('#account-trigger').click();await page.locator('#open-admin').click();
  assert.equal(await page.locator('#invite-form input[name="name"]').inputValue(),'Unfinished invitation','closing preserves unfinished work');
  await page.locator('#close-admin').click();
  assert.equal(await page.locator('header #admin').count(),0,'admin tools occupy no header space');
  const headerBounds=await page.locator('body > header').boundingBox();
  // Ordinary players have neither admin navigation nor the operator entry.
  await context.clearCookies();await context.addCookies([{name:'member',value:'user',url:'https://games.example.com'}]);
  await page.reload();await page.locator('#clubhouse').waitFor();
  assert.equal(await page.locator('#admin').isVisible(),false);
  assert.equal(await page.locator('#open-admin').count(),0);
  assert.deepEqual(await page.locator('body > header').boundingBox(),headerBounds,'admin and player headers have the same layout');
  assert.equal(await page.locator('#operator-groups').count(),0);
  assert.deepEqual(errors,[]);
  console.log('Browser: inspect, cover, read-only chat, return home, mobile, rename and create groups, expiry, operator-only admin menu and focus return passed.');
} catch(error){console.error('Page errors:',errors,'Login:',await page?.locator('body').innerText(),'URL:',page?.url());throw error;} finally {await browser.close();}
