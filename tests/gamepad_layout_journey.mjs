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
];
try {
  const context = await browser.newContext({serviceWorkers:'block'});
  await context.addInitScript(pads => {
    window.fixturePads = pads;
    Object.defineProperty(navigator,'getGamepads',{value:()=>window.fixturePads});
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
    const data = {'/api/me':me, '/api/library':{apps:[app],friends:[me],tags:[]},
      '/api/chat':{messages:[]}, '/api/voice':{participants:[],settings:{}},
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
  await page.getByRole('button',{name:'Play',exact:true}).click();
  const popup = await opening;
  await popup.waitForURL(game+'/_infuser/play');
  async function checkGame() {
    const frame = popup.frameLocator('iframe').locator('body');
    await frame.evaluate(async()=>{await import('/_infuser/sdk/v1.js');});
    assert.deepEqual(await frame.evaluate(()=>fixturePads.map(p=>[Infuser.gamepads.layout(p),Infuser.gamepads.faceButton(p,0)])),
      [['nintendo','B'],['playstation','×'],['nintendo','B']]);
  }
  await checkGame();
  await popup.reload();
  await checkGame();
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
  console.log('PASS: automatic layouts, remembered override, real cross-origin game launch, SDK button labels and reload.');
} finally { await browser.close(); }
