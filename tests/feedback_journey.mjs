// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs node tests/feedback_journey.mjs
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await chromium.launch({headless:true,...(process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
const page=await browser.newPage({viewport:{width:390,height:844}});
let admin=false,available=true,failIntake=false,failPosting=false,entries=[],intakes=[],approvals=[];
const errors=[];page.on('pageerror',e=>errors.push(e.message));
await page.route('https://library.test/**',async route=>{
  const path=new URL(route.request().url()).pathname,method=route.request().method();
  if(path==='/api/me')return route.fulfill({status:401,json:{error:'test'}});
  if(path==='/api/feedback'&&method==='POST'){
    const body=route.request().postDataJSON();intakes.push(body);
    if(!entries.some(e=>e.key===body.request_id))entries.push({id:'queued-'+entries.length,key:body.request_id,created:1700000000,state:'pending',report:body.report});
    if(failIntake){failIntake=false;return route.abort('failed');}
    return route.fulfill({json:{queued:true,id:entries.at(-1).id}});
  }
  if(path==='/api/feedback/review')return route.fulfill({json:{available,entries:entries.filter(e=>!['sent','dismissed'].includes(e.state))}});
  if(path.startsWith('/api/feedback/review/')){
    const entry=entries.find(e=>path.includes(e.id));
    if(method==='DELETE'){entry.state='dismissed';entry.report=null;return route.fulfill({json:{dismissed:true}});}
    approvals.push(route.request().postDataJSON());entry.state=failPosting?'uncertain':'sent';
    return route.fulfill({status:failPosting?502:200,json:failPosting?{error:'Uncertain'}:{submitted:true}});
  }
  const file=path==='/library'?'index.html':path.startsWith('/assets/')?path.slice(8):null;
  if(file&&!file.includes('..'))try{
    let body=await fs.readFile(new URL('../web/'+file,import.meta.url));
    if(file==='clubhouse.js')body=body.toString()+`\nme={name:'Private Name',player_id:'private-id',avatars:[],admin:${admin}};document.getElementById('clubhouse').hidden=false;document.getElementById('welcome').hidden=true;setupAccount();`;
    return route.fulfill({body,contentType:file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':'text/html'});
  }catch{}
  return route.fulfill({status:404,body:''});
});
const dialog=page.locator('#feedback-dialog');
const open=async(name='Send feedback')=>{await page.locator('#account-trigger').click();await page.getByRole('button',{name,exact:true}).click();};
const preview=()=>dialog.locator('.feedback-preview').click();
const consent=()=>dialog.locator('[name=consent]').check();
const close=()=>dialog.getByRole('button',{name:'Close feedback'}).click();
try{
  await page.goto('https://library.test/library?private=secret');await open();
  assert.equal(await page.getByRole('button',{name:'Review feedback',exact:true}).count(),0);
  assert.equal(await dialog.locator('[name=kind]').inputValue(),'feature');
  assert.equal(await dialog.locator('[name=include]').isVisible(),false);
  await dialog.locator('[name=title]').fill('Better search');await dialog.locator('[name=description]').fill('Private draft <script>alert(1)</script>');await preview();
  assert.equal(intakes.length,0);assert.equal(await dialog.locator('.feedback-send').isDisabled(),true);assert.match(await dialog.locator('pre').textContent(),/<script>/);
  failIntake=true;await consent();await dialog.locator('.feedback-send').click();await page.waitForFunction(()=>document.querySelector('.feedback-status').textContent.includes('safely retry'));
  await dialog.locator('.feedback-send').click();await page.waitForFunction(()=>document.querySelector('.feedback-status').textContent.includes('Sent privately'));
  assert.deepEqual(intakes[0],intakes[1]);assert.equal(entries.length,1);assert.equal(approvals.length,0);assert.equal(intakes[0].report.diagnostics,null);
  await close();await page.waitForFunction(()=>document.getElementById('account-trigger')===document.activeElement);await open();
  await dialog.locator('[name=kind]').selectOption('bug');await dialog.locator('[name=title]').fill('Play problem');await dialog.locator('[name=description]').fill('Private game name');await preview();
  assert.match(await dialog.locator('pre').textContent(),/"viewport": "small"/);assert.doesNotMatch(await dialog.locator('pre').textContent(),/Private Name|private-id|library.test|secret/);
  await consent();await dialog.locator('.feedback-send').click();await page.waitForFunction(()=>document.querySelector('.feedback-status').textContent.includes('Sent privately'));
  assert.deepEqual(Object.keys(entries[1].report.diagnostics),['online','secure_context','webgl','webrtc','webassembly','viewport']);
  admin=true;await page.setViewportSize({width:1280,height:900});await page.reload();await open('Review feedback');
  await dialog.getByRole('button',{name:'Review and edit'}).nth(1).click();
  await dialog.locator('[name=description]').fill('A button does not respond.');await preview();
  assert.match(await dialog.locator('.feedback-review pre').textContent(),/"viewport": "small"/,'keep original snapshot, never collect admin device data');
  assert.doesNotMatch(await dialog.locator('.feedback-review pre').textContent(),/Private game name/);
  assert.equal(await dialog.locator('.feedback-send').isDisabled(),true);assert.equal(approvals.length,0);
  await consent();await dialog.locator('.feedback-send').click();await page.waitForFunction(()=>document.querySelector('.feedback-status').textContent.includes('approved public issue'));
  assert.equal(approvals.length,1);assert.equal(approvals[0].public_consent,true);assert.equal(approvals[0].report.description,'A button does not respond.');
  await dialog.locator('.feedback-refresh').click();await dialog.getByRole('button',{name:'Review and edit'}).click();
  await preview();failPosting=true;await consent();await dialog.locator('.feedback-send').click();await page.waitForFunction(()=>document.querySelector('.feedback-status').textContent.includes('could not be confirmed'));
  assert.equal(await dialog.locator('.feedback-send').isDisabled(),true);await dialog.locator('.feedback-refresh').click();
  await page.waitForFunction(()=>document.querySelector('.feedback-entries').textContent.includes('Already attempted'));
  assert.equal(await dialog.getByRole('button',{name:'Review and edit'}).count(),0);
  await dialog.getByRole('button',{name:'Dismiss and delete text'}).click();await page.waitForFunction(()=>document.querySelector('.feedback-status').textContent.includes('No feedback'));
  entries.push({id:'no-token',created:1700000000,state:'pending',report:{kind:'feature',title:'Idea',description:'Please help',diagnostics:null}});available=false;
  await dialog.locator('.feedback-refresh').click();await dialog.getByRole('button',{name:'Review and edit'}).click();await preview();await consent();assert.equal(await dialog.locator('.feedback-send').isDisabled(),true);
  await page.keyboard.press('Escape');await page.waitForFunction(()=>document.getElementById('account-trigger')===document.activeElement);
  assert.deepEqual(errors,[]);console.log('Feedback queue browser journey passed: private intake/retry, role menus, diagnostics, admin editing/consent, posting, uncertainty, dismissal, missing token, mobile and desktop.');
}finally{await browser.close();}
