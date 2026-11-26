// Isolated Library → game origin handoff with real settings, launch shell and SDK.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {ordinaryThemeCalendar} from './support/theme_calendar.mjs';
const {chromium} = await import(process.env.INFUSER_PLAYWRIGHT || 'playwright');
const browser = await chromium.launch({executablePath:process.env.INFUSER_CHROMIUM});
const root = new URL('../', import.meta.url);
const library = 'https://library.test', game = 'https://pond.test';
const pads = [
  {id:'Generic USB adapter', index:0, connected:true, mapping:'standard'},
  {id:'Wireless Controller (Vendor: 054c Product: 09cc)', index:2, connected:true, mapping:'standard'},
  {id:'Pro Controller (057e-2009)', index:3, connected:true, mapping:'standard'}
].map(pad => ({...pad, buttons:Array.from({length:17},()=>({pressed:false})), axes:[0,0]}));
try {
  const context = await browser.newContext({serviceWorkers:'block'});
  await context.addInitScript(pads => {
    window.fixturePads = pads;
    window.padReads = 0;
    Object.defineProperty(navigator,'getGamepads',{value:()=>{ window.padReads++; return window.fixturePads; }});
  }, pads);
  async function asset(route, file) {
    let body = await readFile(new URL(file,root));
    if (file === 'web/themes.js') body = ordinaryThemeCalendar(body.toString());
    return route.fulfill({body,contentType:file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':'text/html'});
  }
  await context.route(library+'/**',async route=>{
    const path = new URL(route.request().url()).pathname;
    if(path === '/library') return asset(route,'web/index.html');
    if(path.startsWith('/assets/')) {
      try { return await asset(route,'web/'+path.slice(8)); } catch { return route.fulfill({status:404}); }
    }
    const me = {name:'Alex',player_id:'self',avatars:[],avatar_prompt_seen:true};
    const app = {id:'pond',title:'Pond',tags:[],players:[],join_targets:[],reactions:[],author:me};
    const data = {'/api/me':me, '/api/library':{apps:[app,{...app,id:'river',title:'River'}],friends:[me],tags:[]},
      '/api/chat':{messages:[]}, '/api/voice':{participants:[],settings:{}},
      '/api/apps/river/launch':{state:'ready',url:game+'/_infuser/launch#infuser=test-ticket'},
      '/api/apps/pond/launch':{state:'ready',url:game+'/_infuser/launch#infuser=test-ticket'}};
    return route.fulfill({json:data[path] || {}});
  });
  await context.route(game+'/**',async route=>{
    const path = new URL(route.request().url()).pathname;
    if(path === '/_infuser/launch') return asset(route,'web/game-launch.html');
    if(path === '/_infuser/session') assert.deepEqual(route.request().postDataJSON(),{token:'test-ticket'});
    if(path === '/_infuser/play') return route.fulfill({contentType:'text/html',body:'<iframe src="/"></iframe>'});
    if(path === '/') return route.fulfill({contentType:'text/html',body:'<script type="module" src="/_infuser/sdk/v1.js"></script>'});
    if(path === '/_infuser/sdk/v1.js') return asset(route,'sdk/v1/infuser.js');
    if(path.endsWith('/gamepads.js')) return asset(route,'web/gamepad-layout.js');
    if(path.startsWith('/_infuser/sdk/v1/')) return asset(route,'sdk/v1/'+path.split('/').pop());
    return route.fulfill({json:{}});
  });
  const page = await context.newPage();
  await page.goto(library+'/library');
  async function settings() {
    await page.locator('#account-trigger').click();
    await page.locator('#open-settings').click();
    await page.locator('#settings-gamepads-region summary').click();
  }
  await settings();
  const selects = page.locator('#settings-gamepads select');
  await selects.first().waitFor();
  assert.deepEqual(await selects.locator('option:checked').allTextContents(),
    ['Automatic (Xbox)','Automatic (PlayStation)','Automatic (Nintendo)']);
  await selects.first().focus();
  assert(await selects.first().evaluate(select => {
    window.dispatchEvent(new Event('gamepadconnected'));
    return document.activeElement === select;
  }), 'an unchanged pad set preserves focus');
  await page.evaluate(()=>{
    fixturePads[0].mapping = '';
    window.dispatchEvent(new Event('gamepadconnected'));
  });
  assert(await page.getByText('Non-standard mapping. Button labels may differ.').isVisible());
  await selects.first().selectOption('nintendo');
  await page.reload();
  await settings();
  assert.equal(await selects.first().inputValue(),'nintendo');
  await page.locator('#close-settings').click();
  await page.evaluate(()=>{
    const realOpen = window.open.bind(window);
    window.open = (...args) => { window.fixturePads = []; return realOpen(...args); };
  });
  const opening = page.waitForEvent('popup');
  await page.getByRole('button',{name:'Play',exact:true}).first().click();
  const popup = await opening;
  await popup.waitForURL(game+'/_infuser/play');
  async function checkGame(gamePage) {
    const frame = gamePage.frameLocator('iframe').locator('body');
    await frame.evaluate(async()=>{await import('/_infuser/sdk/v1.js');});
    assert.deepEqual(await frame.evaluate(()=>fixturePads.map(p=>[Infuser.gamepads.layout(p),Infuser.gamepads.faceButton(p,0)])),
      [['nintendo','B'],['playstation','×'],['nintendo','B']]);
  }
  await checkGame(popup);
  await popup.reload();
  await checkGame(popup);
  await page.evaluate(pads=>{window.fixturePads=pads;},pads);
  await page.locator('#account-trigger').click();
  await page.locator('#open-settings').click();
  await selects.first().selectOption('auto');
  await page.evaluate(()=>{
    window.fixturePads = [];
    window.dispatchEvent(new Event('gamepaddisconnected'));
  });
  assert.equal(await selects.count(),0);
  assert.match(await page.locator('#settings-gamepads').innerText(),/press a button/);
  await page.evaluate(pads=>{
    window.fixturePads = pads;
    window.dispatchEvent(new Event('gamepadconnected'));
  }, pads);
  assert.equal(await selects.first().inputValue(),'auto');
  await selects.first().selectOption('nintendo');
  await popup.close();
  await page.bringToFront();
  await page.locator('#close-settings').click();
  await page.waitForFunction(()=>document.activeElement.id === 'account-trigger');
  await page.setViewportSize({width:1280,height:850});
  await page.evaluate(()=>{
    document.activeElement.blur();
    window.fixturePads = [window.fixturePads[0]];
    window.fixturePads[0].axes = [0.1,0];
    window.dispatchEvent(new Event('gamepadconnected'));
  });
  async function button(index, pressed) {
    const read = await page.evaluate(([index,pressed])=>{
      fixturePads[0].buttons[index].pressed=pressed; return padReads;
    },[index,pressed]);
    await page.waitForFunction(before=>padReads>=before+2,read);
  }
  async function tap(index) { await button(index,true); await button(index,false); }
  const hints=page.locator('.library-pad-hints');
  await page.waitForFunction(()=>window.padReads>3);
  assert.equal(await hints.count(),0,'connection and stick drift do not show hints');
  await tap(12);
  assert.equal(await hints.innerText(),'A · Open');
  const firstCard=await page.evaluate(()=>document.activeElement.closest('.game').dataset.app);
  await button(15,true);
  assert.notEqual(await page.evaluate(()=>document.activeElement.closest('.game').dataset.app),firstCard);
  await page.keyboard.press('Shift');
  await hints.waitFor({state:'detached'});
  await button(15,false);
  assert.equal(await hints.count(),0,'a held direction cannot re-enable hints after a key');
  await page.setViewportSize({width:390,height:850});
  await tap(12);
  assert.equal(await page.evaluate(()=>document.activeElement.closest('.game').dataset.app),firstCard);
  await page.mouse.move(0,0);
  assert(await hints.isVisible(),'mouse movement alone keeps hints');
  await page.mouse.click(0,0);
  await hints.waitFor({state:'detached'});
  await tap(12);
  await tap(1); // Nintendo A is the east face button.
  const dialog=page.locator('#game-detail');
  assert(await dialog.isVisible());
  assert.match(await hints.innerText(),/A · Play.*B · Back/);
  await tap(15); await tap(1);
  assert(await dialog.locator('.play-menu[open]').isVisible());
  await tap(0); assert.equal(await dialog.locator('.play-menu[open]').count(),0);
  await tap(0); assert(!(await dialog.isVisible()));
  await tap(1);
  await page.evaluate(()=>{window.open=()=>{throw Error('controller Play must work with popups blocked');};});
  await page.evaluate(()=>{fixturePads[0].buttons[1].pressed=true;});
  await page.waitForURL(game+'/_infuser/play');
  await checkGame(page);
  assert.equal(await page.locator('.library-pad-hints').count(),0);
  console.log('PASS: Library navigation, layout-aware hints, switching inputs, Play options, Back, same-tab controller launch.');
  console.log('PASS: automatic layouts, remembered override, real cross-origin game launch, SDK button labels and reload.');
} finally { await browser.close(); }
