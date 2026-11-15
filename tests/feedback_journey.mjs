// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs node tests/feedback_journey.mjs
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await chromium.launch({headless:true,...(process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
const page=await browser.newPage({viewport:{width:390,height:844}});
let available=true, failure=0, posts=[];
const errors=[];page.on('pageerror',e=>errors.push(e.message));
await page.route('https://library.test/**',async route=>{
  const path=new URL(route.request().url()).pathname;
  if(path==='/api/me')return route.fulfill({status:401,json:{error:'test'}});
  if(path==='/api/feedback'){
    if(route.request().method()==='GET')return route.fulfill({json:{available}});
    posts.push(route.request().postDataJSON());
    if(failure===-1)return route.abort('failed');
    return route.fulfill({status:failure||200,json:failure?{error:'Feedback limit reached.'}:{submitted:true}});
  }
  const file=path==='/library'?'index.html':path.startsWith('/assets/')?path.slice(8):null;
  if(file&&!file.includes('..'))try{
    let body=await fs.readFile(new URL('../web/'+file,import.meta.url));
    if(file==='clubhouse.js')body=body.toString()+`\nme={name:'Private Name',player_id:'private-id',avatars:[]};document.getElementById('clubhouse').hidden=false;document.getElementById('welcome').hidden=true;setupAccount();`;
    return route.fulfill({body,contentType:file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':'text/html'});
  }catch{}
  return route.fulfill({status:404,body:''});
});
const dialog=page.locator('#feedback-dialog');
const open=async()=>{await page.locator('#account-trigger').click();await page.getByRole('button',{name:'Send feedback',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('.feedback-status').textContent.includes('Checking'));};
const review=async()=>{await dialog.getByRole('button',{name:'Review public issue'}).click();};
const consent=()=>dialog.locator('[name=consent]').check();
try{
  await page.goto('https://library.test/library?private=secret');await open();
  assert.equal(await dialog.locator('[name=kind]').inputValue(),'feature');
  assert.equal(await dialog.locator('[name=include]').isVisible(),false);
  await dialog.locator('[name=title]').fill('Better search');await dialog.locator('[name=description]').fill('Add a filter <script>alert(1)</script>');
  await review();assert.equal(posts.length,0);assert.equal(await dialog.locator('.feedback-send').isDisabled(),true);
  assert.match(await dialog.locator('pre').textContent(),/<script>/);
  await consent();await dialog.locator('.feedback-send').click();await page.waitForFunction(()=>document.querySelector('.feedback-status').textContent.includes('was posted'));
  assert.equal(posts.length,1);assert.equal(posts[0].diagnostics,null);assert.equal(posts[0].public_consent,true);
  await dialog.getByRole('button',{name:'Close feedback'}).click();await open();
  assert.equal(await dialog.locator('[name=title]').inputValue(),'');
  await dialog.locator('[name=kind]').selectOption('bug');await dialog.locator('[name=title]').fill('Play problem');await dialog.locator('[name=description]').fill('The button does not work.');
  await review();const preview=await dialog.locator('pre').textContent();assert.match(preview,/"viewport": "small"/);assert.doesNotMatch(preview,/Private Name|private-id|library.test|secret/);
  await dialog.locator('.feedback-edit').click();await dialog.locator('[name=include]').uncheck();await review();assert.doesNotMatch(await dialog.locator('pre').textContent(),/diagnostics/);
  await dialog.locator('.feedback-edit').click();await dialog.locator('[name=include]').check();await review();
  failure=429;await consent();await dialog.locator('.feedback-send').click();await page.waitForFunction(()=>document.querySelector('.feedback-status').textContent.includes('limit reached'));
  assert.equal(posts.length,2);assert.deepEqual(Object.keys(posts[1].diagnostics),['online','secure_context','webgl','webrtc','webassembly','viewport']);
  assert.match(await dialog.locator('pre').textContent(),/Play problem/);
  failure=502;await dialog.locator('.feedback-send').click();await page.waitForFunction(()=>document.querySelector('.feedback-status').textContent.includes('could not confirm'));
  assert.equal(posts.length,3);assert.equal(await dialog.locator('.feedback-send').isDisabled(),true);assert.equal(await dialog.locator('.feedback-edit').isDisabled(),true);
  await page.keyboard.press('Escape');assert.equal(await dialog.isVisible(),false);await page.waitForFunction(()=>document.getElementById('account-trigger')===document.activeElement);
  await open();assert.match(await dialog.locator('.feedback-status').textContent(),/could not confirm/);assert.equal(await dialog.locator('.feedback-send').isDisabled(),true);
  await page.reload();available=false;await open();await dialog.locator('[name=title]').fill('Draft');await dialog.locator('[name=description]').fill('Keep this');await review();await consent();assert.equal(await dialog.locator('.feedback-send').isDisabled(),true);
  assert.equal(await dialog.evaluate(e=>e.getBoundingClientRect().right<=innerWidth),true);
  await page.setViewportSize({width:1280,height:900});available=true;failure=-1;await page.reload();await open();
  await dialog.locator('[name=title]').fill('Network failure');await dialog.locator('[name=description]').fill('Keep the reviewed draft');await review();await consent();await dialog.locator('.feedback-send').click();await page.waitForFunction(()=>document.querySelector('.feedback-status').textContent.includes('could not confirm'));assert.equal(await dialog.locator('.feedback-send').isDisabled(),true);
  assert.deepEqual(errors,[]);console.log('Feedback browser journey passed: feature, bug, privacy, consent, success, limits, uncertain outcome, setup, mobile, Escape.');
}finally{await browser.close();}
