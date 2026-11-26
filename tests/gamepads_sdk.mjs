import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import {automaticLayout} from '../web/gamepad-layout.js';
import {captureGamepadLayouts, gamepadLaunchUrl, preference} from '../web/gamepads.js';

test('browser model names, Chromium vendor fields and Firefox vendor-product-name IDs', () => {
  for (const [family, ids] of Object.entries({
    xbox: ['Xbox 360 Controller (XInput STANDARD GAMEPAD)', '045e-028e-Microsoft X-Box 360 pad',
      '2dc8-054c-Some Pad', '2dc8-057e-Some Pad', 'Some Pad (Vendor: 2dc8 Product: 054c)',
      'Some Pad (2dc8-057e)', 'Some Pad (Vendor: 057ea Product: 0001)', 'USB Gamepad', ''],
    playstation: ['Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 09cc)',
      '054c-09cc-Sony Interactive Entertainment Wireless Controller',
      'Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)',
      '054c-0ce6-Sony Interactive Entertainment DualSense Wireless Controller',
      '54c-5c4-Wireless Controller', 'PS4 Controller', 'PS5 Controller', 'DualSense', 'DUALSHOCK 4'],
    nintendo: ['Pro Controller (STANDARD GAMEPAD Vendor: 057e Product: 2009)',
      'Pro Controller (057e-2009)', '057e-2009-Pro Controller', '57e-2009-Nintendo Switch Pro Controller', 'Joy-Con (L)']
  })) for (const id of ids) assert.equal(automaticLayout(id), family, id);
});

const key = 'infuser.launch.pad-layouts';
let instance = 0;
function mockStorage(t, name, value) {
  const previous = Object.getOwnPropertyDescriptor(globalThis,name);
  Object.defineProperty(globalThis,name,{configurable:true,value});
  t.after(()=>{
    if (previous) Object.defineProperty(globalThis,name,previous);
    else delete globalThis[name];
  });
}
async function loadSdk() {
  const {gamepads} = await import('../web/gamepad-layout.js?instance=' + ++instance);
  return gamepads;
}
test('face labels require standard mapping even when the family was selected manually', async t => {
  mockStorage(t, 'sessionStorage', {getItem:()=>'[["Adapter","playstation"]]'});
  const sdk = await loadSdk();
  for (const mapping of ['', 'xr-standard', undefined]) {
    const pad = {id:'Adapter', mapping};
    assert.equal(sdk.layout(pad), 'playstation');
    assert.equal(sdk.faceButton(pad,0), null);
  }
  const pad = {id:'Adapter',mapping:'standard'};
  assert.deepEqual([0,1,2,3].map(i=>sdk.faceButton(pad,i)), ['×','○','□','△']);
  for (const i of [-1,4,0.5,'0']) assert.equal(sdk.faceButton(pad,i),null);
  assert.equal(sdk.faceButton(null,0),null);
});

test('malformed or oversized session preferences fall back atomically',async t=>{
  let raw;
  mockStorage(t, 'sessionStorage', {getItem:k=>k===key?raw:null});
  for (raw of ['{', '{}', '[["Pad","nintendo"],null]', '[["Pad","__proto__"]]',
    '[["Pad","nintendo"]]' + ' '.repeat(32768)]) {
    const sdk = await loadSdk();
    assert.equal(sdk.layout({id:'Pad'}),'xbox');
  }
});

test('local preferences and optional launch decoration tolerate unusable data', t=>{
  let saved = 'nintendo';
  mockStorage(t, 'localStorage', {getItem:()=>saved});
  const original = 'https://game.test/_infuser/launch#infuser=ticket';
  mockStorage(t, 'navigator', {getGamepads:()=>[{id:'Adapter',connected:true}]});
  const snapshot = captureGamepadLayouts();
  assert.deepEqual(JSON.parse(snapshot),[['Adapter','nintendo']]);
  const fragment = new URLSearchParams(new URL(gamepadLaunchUrl(original,snapshot)).hash.slice(1));
  assert.equal(fragment.get('infuser'),'ticket');
  assert.equal(fragment.get('pad-layouts'),snapshot);
  assert.equal(gamepadLaunchUrl('/relative',snapshot),'/relative');
  for (saved of ['__proto__','["xbox"]','x'.repeat(32769)]) {
    assert.equal(preference('Adapter'),'auto');
    assert.equal(gamepadLaunchUrl(original,captureGamepadLayouts()),original);
  }
});

const launchScript = readFileSync(new URL('../web/game-launch.html',import.meta.url),'utf8').match(/<script>([\s\S]*?)<\/script>/)[1];
test('launch shell stores at most 32768 decoded characters and still opens Play', async()=>{
  for (const length of [32768,32769]) {
    const raw = ' '.repeat(length-2)+'[]';
    const values = new Map([[key,'stale']]);
    let destination;
    await vm.runInNewContext(launchScript,{
      URLSearchParams, history:{replaceState(){}}, fetch:async()=>({ok:true}),
      location:{search:'', hash:'#'+new URLSearchParams({infuser:'ticket','pad-layouts':raw}), replace:path=>{destination=path;}},
      sessionStorage:{setItem:(k,v)=>values.set(k,v), removeItem:k=>values.delete(k)}
    });
    assert.equal(values.get(key),length===32768?raw:undefined);
    assert.equal(destination,'/_infuser/play');
  }
});
