// Runs against the real temporary Rust HTTP server in compatibility_journey.rs.
import assert from 'node:assert/strict';
import http from 'node:http';
import { pathToFileURL } from 'node:url';
const endpoint=process.argv[2],host='games.example.com';
const outstanding=new Set(), timers=new Map(), errors=[];
let serial=0;
globalThis.window={dispatchEvent:event=>errors.push(event.detail)};
globalThis.setInterval=callback=>{const id=++serial;timers.set(id,callback);return id;};
globalThis.clearInterval=id=>timers.delete(id);
function transport(path,{method='GET',headers={},body}={}){
 return new Promise((resolve,reject)=>{
  const request=http.request(endpoint+path,{method,headers},response=>{
   const chunks=[];response.on('data',chunk=>chunks.push(chunk));response.on('end',()=>resolve(new Response(Buffer.concat(chunks),{status:response.statusCode,headers:response.headers})));
  });request.on('error',reject);request.end(body);
 });
}
async function json(path,options){const r=await transport(path,options);const v=await r.json();assert.ok(r.ok,JSON.stringify(v));return v;}
const memberHeaders={Host:host,Origin:'https://'+host,Cookie:'member=admin','Content-Type':'application/json'};
async function open(guest,versioned=false,previous){
 const launch=previous||await json('/api/apps/pond/launch',{method:'POST',headers:memberHeaders,body:JSON.stringify({guest})});
 const u=new URL(launch.url),token=new URLSearchParams(u.hash.slice(1)).get('infuser');
 const headers={Host:u.host,Origin:u.origin,Authorization:'Bearer '+token};
 if(!previous)await json('/_infuser/session',{method:'POST',headers:{...headers,'Content-Type':'application/json'},body:JSON.stringify({token})});
 const paths=[];
 globalThis.fetch=(path,options={})=>{
  paths.push(path);const pending=transport(path,{...options,headers:{...headers,...options.headers}});outstanding.add(pending);pending.finally(()=>outstanding.delete(pending));return pending;
 };
 let source;
 if(versioned){
  // Node cannot import HTTPS modules. Load the actual served module bytes and
  // resolve the core's cloud import to those bytes, without changing SDK logic.
  const cloud=await transport('/_infuser/sdk/v1/cloud.js',{headers});assert.equal(cloud.status,200);
  const observer=await transport('/_infuser/sdk/v1/session-fetch.js',{headers});assert.equal(observer.status,200);
  const observerUrl='data:text/javascript;base64,'+Buffer.from(await observer.text()).toString('base64');
  const cloudUrl='data:text/javascript;base64,'+Buffer.from((await cloud.text()).replace("'./session-fetch.js'",JSON.stringify(observerUrl))).toString('base64');
  const r=await transport('/_infuser/sdk/v1.js',{headers});assert.equal(r.status,200);
  source='data:text/javascript;base64,'+Buffer.from((await r.text()).replace("'/_infuser/sdk/v1/cloud.js'",JSON.stringify(cloudUrl)).replace("'/_infuser/sdk/v1/session-fetch.js'",JSON.stringify(observerUrl))).toString('base64')+'#'+(++serial);
 }
 else source=pathToFileURL(new URL('./fixtures/legacy-sdk/infuser.js',import.meta.url).pathname).href+'?instance='+(++serial);
 const sdk=await import(source);const context=await sdk.ready;await Promise.all(outstanding);
 assert.equal(context.guest,guest);assert.equal(context.app_id,'pond');assert.ok(context.player_id);
 for(const tick of timers.values())await tick();
 assert.ok(paths.some(p=>p.endsWith('/heartbeat')),'the unchanged SDK must renew its session');
 if(versioned){assert.equal(sdk.version.major,1);assert.ok(paths.every(p=>p.startsWith('/_infuser/api/v1/')));}
 return {sdk,context,launch,paths};
}
let game=await open(false);const owner=game.context.player_id;
assert.deepEqual(await game.sdk.saves.load('progress'),{version:0,data:null});
await game.sdk.saves.save('progress',{level:13},0);await game.sdk.leave();
game=await open(false);assert.equal(game.context.player_id,owner);assert.equal((await game.sdk.saves.load('progress')).data.level,13);await game.sdk.leave();
game=await open(true);const firstGuest=game.context.player_id;assert.notEqual(firstGuest,owner);
assert.equal((await game.sdk.saves.load('progress')).data,null);await game.sdk.saves.save('progress',{level:99},0);
// Reload the same guest launch: its ephemeral save must survive, without merging.
const original=game.sdk;game=await open(true,false,game.launch);assert.equal(game.context.player_id,firstGuest);assert.equal((await game.sdk.saves.load('progress')).data.level,99);await game.sdk.leave();timers.clear();
game=await open(true);assert.notEqual(game.context.player_id,firstGuest);assert.equal((await game.sdk.saves.load('progress')).data,null);await game.sdk.leave();
game=await open(false,true);assert.equal(game.context.player_id,owner);assert.equal((await game.sdk.saves.load('progress')).data.level,13);await game.sdk.saves.save('progress',{level:14},1);
// Follow the documented main-import workflow, not the standalone cloud module.
const bytes=Uint8Array.from([0,255,128,1]);
const binary=game.sdk.cloud.encodeBinary(bytes);
assert.deepEqual(binary,{base64:'AP+AAQ=='});
assert.deepEqual(window.Infuser.cloud.decodeBinary(binary),bytes);
assert.throws(()=>game.sdk.cloud.decodeBinary({base64:'invalid'}),error=>error.code==='invalid_binary_file');
const empty=await game.sdk.cloud.load('campaign');assert.equal(empty.revision,null);
const checkpoint={expectedRevision:empty.revision,commitId:'compat-binary-checkpoint-0001',files:{'progress.json':{level:14},'world.bin':binary}};
const saved=await game.sdk.cloud.commit('campaign',checkpoint);assert.equal(saved.status,'saved');
assert.deepEqual(await game.sdk.cloud.commit('campaign',checkpoint),saved);
const loaded=await game.sdk.cloud.load('campaign');assert.equal(loaded.files['progress.json'].level,14);assert.deepEqual(game.sdk.cloud.decodeBinary(loaded.files['world.bin']),bytes);
assert.ok(game.paths.every(p=>p.startsWith('/_infuser/api/v1/')));await game.sdk.leave();
game=await open(false);assert.equal((await game.sdk.saves.load('progress')).data.level,14);await game.sdk.leave();
assert.deepEqual(errors,[]);console.log('Frozen legacy SDK and versioned SDK: identity, save/reload, guest refresh/isolation, renewal, shared save ownership and main-import binary checkpoints passed.');
