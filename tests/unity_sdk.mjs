import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import vm from 'node:vm';
import {createBridge} from '../sdk/unity/Assets/Infuser/Web/bridge.mjs';
import {cloud} from '../sdk/v1/cloud.js';

function harness(api) {
  const replies = [], events = [];
  const bridge = createBridge(m => (m.event ? events : replies).push(m), typeof api === 'function' ? api : async () => api);
  let id = 0;
  return {bridge, events, async call(op, args = {}) {
    const key = String(++id);
    await bridge.call(key, op, JSON.stringify(args));
    const r = replies.find(r => r.id === key);
    return r?.ok ? {ok:true, value:JSON.parse(r.json)} : r;
  }};
}
const identity = {guest:false, player_id:'test-player', name:'Player'};

test('standalone is explicit; a hosted authentication failure never becomes standalone', async () => {
  const local = harness(null);
  assert.deepEqual((await local.call('ready')).value, {hosted:false});
  assert.equal((await local.call('save.load', {group:'progress', file:'progress.json'})).code, 'not_hosted');
  const hosted = harness({ready:Promise.resolve(identity)});
  assert.deepEqual((await hosted.call('ready')).value, {hosted:true,...identity});
  const denied = harness(async () => { throw Object.assign(Error('Launch expired'), {status:401}); });
  const r = await denied.call('ready');
  assert.equal(r.ok, false); assert.equal(r.status, 401); assert.equal(r.error, 'Launch expired');
  for (const h of [local, hosted, denied]) h.bridge.dispose();
});

test('single-file cloud bridge uses the existing retry/conflict contract, including binary escape hatch', async () => {
  const original = globalThis.fetch;
  const h = harness({ready:Promise.resolve(identity), cloud});
  const calls = [];
  try {
    globalThis.fetch = async (url, options) => {
      calls.push({url, body:options.body});
      if (!options.body) return Response.json({revision:'r1', files:{'progress.json':{schema:1,stars:4}}, summary:'4 stars',created_at:123});
      if (calls.length === 2) throw Error('Lost acknowledgement');
      return Response.json({status:'conflict',current_revision:'r2',candidate_revision:'candidate'},{status:409});
    };
    const loaded = (await h.call('save.load',{group:'progress',file:'progress.json'})).value;
    assert.equal(JSON.parse(loaded.json).stars, 4); assert.equal(loaded.revision,'r1');
    const args = {group:'progress',file:'progress.json',expectedRevision:'r1',commitId:'stable-checkpoint-123',json:'{"schema":1,"stars":5}',summary:'5 stars'};
    const result = (await h.call('save.commit',args)).value;
    assert.equal(result.status,'conflict'); assert.equal(calls[1].body,calls[2].body);
    assert.deepEqual(JSON.parse(calls[1].body).files, {'progress.json':{schema:1,stars:5}});
    await h.call('save.commit',{...args,commitId:'resolve-checkpoint-123',expectedRevision:'r2',candidateRevision:'candidate'});
    assert.equal(JSON.parse(calls.at(-1).body).resolve_candidate,'candidate');
    globalThis.fetch = async () => Response.json({revision:'r3',files:{'a.json':{},'b.json':{}}});
    assert.equal((await h.call('save.load',{group:'progress',file:'a.json'})).code,'save_layout');
    globalThis.fetch = async (_url,options) => { calls.push({body:options.body});return Response.json({status:'saved',revision:'r4'}); };
    const checkpoint = {expectedRevision:null,commitId:'binary-checkpoint-123',files:{'world.bin':cloud.encodeBinary(Uint8Array.of(0,255,128))}};
    assert.equal((await h.call('cloud.commit',{group:'world',checkpoint})).value.status,'saved');
    assert.deepEqual(JSON.parse(calls.at(-1).body).files, checkpoint.files);
    assert.equal((await h.call('save.commit',{...args,json:'invalid'})).ok,false);
  } finally { h.bridge.dispose(); globalThis.fetch = original; }
});

test('Unity Host uses explicit room creation', async () => {
  let options;
  const h=harness({peers:{create:async o=>{options=o;return {role:'host',epoch:'room',close(){}};}}});
  try {
    assert.equal((await h.call('peer.create',{joinable:true})).value.role,'host');
    assert.equal(options.joinable,true);
    assert.equal(options.sessionId,undefined);
  } finally {h.bridge.dispose();}
});

test('Join Friend keeps its exact target, forwards peer events, and sends without implicit join', async () => {
  let options, closed=0, sent, joinable;
  const room = {id:'self',hostId:'host',role:'member',epoch:'epoch',active:true,
    send:(to,data) => { sent={to,data}; return false; }, broadcast:data => { sent=data; },
    setJoinable:async v => { joinable=v; return v; },close:()=>{closed++;}};
  const h=harness({ready:Promise.resolve(identity),peers:{join:async o => {options=o;return room;}}});
  try {
    assert.equal((await h.call('peer.send',{to:'host',json:'{}'})).code,'not_connected');
    const intent={kind:'join_friend',session_id:'exact-session',player_id:'exact-player'};
    assert.equal((await h.call('peer.join',{intent,relayOnly:true})).value.role,'member');
    assert.equal(options.sessionId,'exact-session');assert.equal(options.playerId,'exact-player');assert.equal(options.relayOnly,true);
    assert.equal(options.joinable,false);
    options.onMessage('host',{type:'state',tick:2});
    const event=JSON.parse(h.events.at(-1).json);assert.equal(event.from,'host');assert.equal(JSON.parse(event.json).tick,2);
    assert.equal((await h.call('peer.send',{to:'host',json:'{"type":"input"}'})).value.sent,false);
    assert.deepEqual(sent,{to:'host',data:{type:'input'}});
    await h.call('peer.joinable',{joinable:true}); assert.equal(joinable,true);
    assert.equal((await h.call('peer.join')).code,'already_connected');
    await h.call('network.close'); assert.equal(closed,1);
    const count=h.events.length;options.onMessage('host',{});assert.equal(h.events.length,count);
  } finally {h.bridge.dispose();}
  let attempts=0;
  const failed=harness({peers:{join:async()=>{attempts++;throw Object.assign(Error('Friend left'),{code:'target_gone',status:410});}}});
  assert.equal((await failed.call('peer.join',{intent:{kind:'join_friend',session_id:'s',player_id:'p'}})).status,410);
  assert.equal(attempts,1);failed.bridge.dispose();
});

test('leaving while admission is pending fences late rooms and blocks overlapping joins', async () => {
  let resolve, closed=0;
  const h=harness({peers:{join:()=>new Promise(r=>{resolve=r;})}});
  const pending=h.call('peer.join');
  await new Promise(r=>setImmediate(r));
  assert.equal((await h.call('peer.join')).code,'already_connected');
  await h.call('network.close');
  resolve({close:()=>{closed++;}});
  assert.equal((await pending).code,'cancelled'); assert.equal(closed,1);h.bridge.dispose();
});

test('dedicated WebSockets carry text and binary, preserve admission, and report congestion', async () => {
  const socket=new EventTarget();let sent, options, disconnected=0, released=0;
  Object.assign(socket,{readyState:1,bufferedAmount:0,infuser:{attempt_id:'a',lease_id:'lease',session:{id:'room'}},send:v=>{sent=v;},close:()=>{socket.readyState=3;}});
  const h=harness({cloud,runtime:{release:async id=>{assert.equal(id,'lease');released++;}},sessions:{connect:async o=>{options=o;return socket;},disconnect:async s=>{assert.equal(s,socket);disconnected++;s.close();}}});
  const intent={kind:'join_friend',session_id:'s',player_id:'p'};
  assert.deepEqual((await h.call('socket.connect',{intent})).value,socket.infuser);
  assert.deepEqual(options.intent,intent);assert.equal(options.binaryType,'arraybuffer');
  socket.dispatchEvent(new MessageEvent('message',{data:'welcome'}));assert.equal(JSON.parse(h.events.at(-1).json).text,'welcome');
  socket.dispatchEvent(new MessageEvent('message',{data:Uint8Array.of(0,255).buffer}));assert.equal(JSON.parse(h.events.at(-1).json).base64,'AP8=');
  assert.equal((await h.call('socket.send',{base64:'AP8='})).value.sent,true);assert.deepEqual(sent,Uint8Array.of(0,255));
  socket.bufferedAmount=65537;assert.equal((await h.call('socket.send',{text:'input'})).value.sent,false);
  // Unexpected loss retains the reservation for explicit resume; leaving from
  // the disconnected screen must still cancel it and release the runtime.
  socket.dispatchEvent(new Event('close'));assert.equal(released,0);
  await h.call('network.close');assert.equal(disconnected,1);assert.equal(released,1);h.bridge.dispose();
});

test('cancelled dedicated admission releases a late socket and suppresses late events', async () => {
  let resolve, released=0, disconnected=0;
  const socket=new EventTarget();socket.infuser={lease_id:'lease'};
  const h=harness({runtime:{release:async()=>{released++;}},sessions:{connect:()=>new Promise(r=>{resolve=r;}),disconnect:async()=>{disconnected++;}}});
  const pending=h.call('socket.connect');await new Promise(r=>setImmediate(r));
  await h.call('network.close');resolve(socket);
  assert.equal((await pending).code,'cancelled');assert.equal(disconnected,1);assert.equal(released,1);
  assert.equal(h.events.length,0);h.bridge.dispose();
});

test('deliberate leave releases the runtime even when disconnect acknowledgement fails', async () => {
  let released=0;const socket=new EventTarget();socket.infuser={lease_id:'lease'};
  const h=harness({runtime:{release:async()=>{released++;}},sessions:{connect:async()=>socket,disconnect:async()=>{throw Error('Lost acknowledgement');}}});
  await h.call('socket.connect');assert.equal((await h.call('network.close')).ok,false);
  assert.equal(released,1);h.bridge.dispose();
});

test('new joins wait for leave to release the old launch lease', async () => {
  let release, released=0;const socket=new EventTarget();socket.infuser={lease_id:'lease'};
  const h=harness({runtime:{release:()=>new Promise(r=>{release=()=>{released++;r();};})},sessions:{connect:async()=>socket,disconnect:async()=>{}}});
  await h.call('socket.connect');const leave=h.call('network.close');
  await new Promise(r=>setImmediate(r));
  assert.equal((await h.call('socket.connect')).code,'already_connected');
  const repeated=h.call('network.close');release();
  assert.equal((await leave).ok,true);assert.equal((await repeated).ok,true);assert.equal(released,1);
  h.bridge.dispose();
});

test('Unity jslib copies strings before asynchronous loading, reports missing bridge, and disposes safely', async () => {
  const source=await fs.readFile(new URL('../sdk/unity/Assets/Infuser/Plugins/WebGL/Infuser.jslib',import.meta.url),'utf8');
  // Emscripten's library parser requires ES5 declarations/functions here.
  assert.doesNotMatch(source,/\b(?:const|let|async|await|import)\b|=>/);
  let script, bridge, calls=[];const library={}, strings={1:'receiver',2:'request',3:'ready',4:'{}'};
  const context=vm.createContext({LibraryManager:{library},mergeInto:Object.assign,Promise,URL,
    UTF8ToString:p=>strings[p],SendMessage:(...a)=>calls.push(a),
    document:{baseURI:'https://game.example/export/index.html',createElement:()=>({}),head:{appendChild:s=>{script=s;}}},
    InfuserUnity:{createBridge:send=>(bridge={call:async(id,op,json)=>send({id,ok:true,json:JSON.stringify({op,json})}),dispose:()=>{bridge.disposed=true;}})}});
  vm.runInContext(source,context);context.InfuserUnityPlugin=library.$InfuserUnityPlugin;
  library.Infuser_Call(1,2,3,4);strings[2]='changed';strings[3]='changed';
  assert.equal(script.src,'https://game.example/export/infuser-unity.mjs');script.onload();
  await new Promise(r=>setImmediate(r));
  assert.equal(JSON.parse(calls[0][2]).id,'request');assert.equal(JSON.parse(JSON.parse(calls[0][2]).json).op,'ready');
  library.Infuser_Dispose();assert.equal(bridge.disposed,true);
  context.InfuserUnityPlugin={loading:null,bridge:null,disposed:false};calls=[];
  library.Infuser_Call(1,2,3,4);script.onerror();await new Promise(r=>setImmediate(r));
  assert.equal(JSON.parse(calls[0][2]).code,'bridge_unavailable');
});
