// Fast lifecycle journeys through the shipped SDK, with a deterministic clock and
// controllable signaling/WebRTC boundaries. No browser install or real-time sleeps.
import baseTest from 'node:test';
import assert from 'node:assert/strict';
import { setImmediate } from 'node:timers/promises';
import { join as legacyJoin } from '../sdk/peers.js';
import { create, join as versionedJoin } from '../sdk/v1/peers.js';
for(const [variant,join] of [['legacy',legacyJoin],['v1',versionedJoin]]) {
const test=(name,fn)=>baseTest(variant+': '+name,fn);

const deferred = () => {
  let resolve;
  const promise = new Promise(r => { resolve = r; });
  return {promise, resolve};
};
function setup(t, role = 'host') {
  let clock = 1000, nextPoll, offerGate, answerGate;
  const requests = [], connections = [], endings = [], states = [], messages = [], indicators = [];
  const snapshot = {self_id:'self', host_id:role === 'host' ? 'self' : 'friend', role,
    epoch:'session-one', lease_ms:25000, peers:[{id:'self'},{id:'friend'}], signals:[],
    ice_servers:[], ice_transport_policy:'relay', accepting_players:true};
  t.mock.timers.enable({apis:['setTimeout','setInterval']});
  t.mock.method(performance, 'now', () => clock);
  const install = (key, value) => {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, key);
    Object.defineProperty(globalThis, key, {configurable:true, writable:true, value});
    t.after(() => descriptor ? Object.defineProperty(globalThis,key,descriptor) : delete globalThis[key]);
  };
  install('window', {parent:{postMessage:m => indicators.push(m.status)}});
  install('location', {origin:'https://game.example.com'});
  install('fetch', async (path, options) => {
    requests.push({path, method:options.method, body:options.body && JSON.parse(options.body)});
    if (options.method === 'GET' && nextPoll) {
      const pending = nextPoll; nextPoll = undefined; return pending;
    }
    return Response.json(snapshot);
  });
  install('RTCPeerConnection', class {
    constructor(config) { this.config=config; this.connectionState='new'; this.signalingState='stable'; connections.push(this); }
    createDataChannel() {
      return this.dc = {readyState:'connecting',bufferedAmount:0,sent:[],send(data) { this.sent.push(data); },close() { this.readyState='closed'; }};
    }
    async createOffer() { if (offerGate) { const g=offerGate; offerGate=undefined; await g; } return {type:'offer'}; }
    async createAnswer() { if (answerGate) { const g=answerGate; answerGate=undefined; await g; } return {type:'answer'}; }
    async setLocalDescription(description) { this.localDescription={...description,toJSON:()=>description}; this.signalingState='have-local-offer'; }
    async setRemoteDescription(description) { this.remoteDescription=description; }
    async addIceCandidate() {}
    async getStats() { return new Map(); }
    close() { this.connectionState='closed'; this.closed=true; if(this.dc)this.dc.readyState='closed'; }
  });
  return {snapshot,requests,connections,endings,states,messages,indicators,
    start:(options={})=>join({...options,onEnd:r=>endings.push(r),onState:s=>states.push(s),onMessage:(...m)=>messages.push(m)}),
    poll:promise=>{nextPoll=promise;}, offer:promise=>{offerGate=promise;}, answer:promise=>{answerGate=promise;},
    async advance(ms=0) { clock+=ms; t.mock.timers.tick(ms); await setImmediate(); },
    open(pc) { pc.connectionState='connected'; pc.dc.readyState='open'; pc.dc.onopen(); },
  };
}

if (variant === 'v1') test('Host creates an independent open room and rejects a join target', async t => {
  const h=setup(t);
  await assert.rejects(create({sessionId:'a'.repeat(32)}), /target/);
  assert.equal(h.requests.length,0);
  const room=await create();
  assert.equal(h.requests[0].body.create,true);
  assert.equal(h.requests[0].body.joinable,true);
  room.close();
});

test('connection preference permits direct paths in v1; explicit relay tests stay relay-only', async t => {
  const h=setup(t);
  const room=await h.start();
  assert.equal(h.requests[0].body.relay_only,variant==='legacy');
  room.close();
  const forced=await h.start({relayOnly:true});
  assert.equal(h.requests.filter(r=>r.path.endsWith('/peers')).at(-1).body.relay_only,true);
  forced.close();
});

test('leaving while a poll is in flight cannot reopen connections or the indicator', async t => {
  const h=setup(t), pending=deferred();
  h.poll(pending.promise);
  const room=await h.start(); await h.advance();
  room.close(); const count=h.states.length;
  pending.resolve(Response.json(h.snapshot)); await h.advance();
  assert.equal(h.connections.length,0);
  assert.equal(h.states.length,count);
  assert.equal(h.indicators.at(-1),'ended');
  assert.equal(h.endings.length,1);
});

test('leaving during host negotiation cannot send a late offer', async t => {
  const h=setup(t), pending=deferred(); h.offer(pending.promise);
  const room=await h.start(); await h.advance();
  assert.equal(h.connections.length,1);
  room.close(); pending.resolve(); await h.advance();
  assert.equal(h.connections[0].closed,true);
  assert.equal(h.requests.filter(r=>r.path.endsWith('/signals')).length,0);
  assert.equal(h.indicators.at(-1),'ended');
});

test('leaving during member negotiation cannot send a late answer', async t => {
  const h=setup(t,'member'), pending=deferred(); h.answer(pending.promise);
  h.snapshot.signals=[{sequence:1,from:'friend',payload:{type:'offer',generation:'first',description:{type:'offer'}}}];
  const room=await h.start(); await h.advance();
  room.close(); pending.resolve(); await h.advance();
  assert.equal(h.connections[0].closed,true);
  assert.equal(h.requests.filter(r=>r.path.endsWith('/signals')).length,0);
  assert.equal(h.indicators.at(-1),'ended');
});

test('tab sleep fences negotiation and rejects a late data channel before the watchdog runs', async t => {
  const h=setup(t),pending=deferred(); h.offer(pending.promise);
  const room=await h.start(); await h.advance();
  // A resumed tab may deliver a promise/event before its overdue watchdog timer.
  t.mock.method(performance,'now',()=>30000);
  pending.resolve(); await setImmediate();
  const channel={closed:false,close(){this.closed=true;}};
  h.connections[0].ondatachannel({channel});
  assert.equal(channel.closed,true); assert.equal(room.active,false);
  assert.equal(h.requests.filter(r=>r.path.endsWith('/signals')).length,0);
  await h.advance(500); assert.equal(h.endings.length,1);
  assert.equal(h.connections[0].closed,true);
});

test('brief signaling failure recovers in place; expired lease fences packets and late replies', async t => {
  const h=setup(t), room=await h.start(); await h.advance();
  const pc=h.connections[0]; h.open(pc);
  assert.equal(room.send('friend',{tick:1}),true);
  h.poll(Promise.resolve(Response.json({error:'temporarily unavailable'},{status:503})));
  await h.advance(1000); assert.equal(room.active,true); assert.equal(h.endings.length,0);
  await h.advance(1000); assert.equal(room.epoch,'session-one'); assert.equal(h.connections.length,1);
  const pending=deferred(); h.poll(pending.promise); await h.advance(1000);
  await h.advance(25001);
  assert.equal(room.active,false); assert.equal(room.send('friend',{tick:2}),false);
  pc.dc.onmessage({data:'{"tick":3}'}); assert.deepEqual(h.messages,[]);
  pending.resolve(Response.json(h.snapshot)); await h.advance();
  assert.equal(h.endings.length,1); assert.equal(h.indicators.at(-1),'ended');
  assert.equal(h.requests.filter(r=>r.method==='POST'&&r.path.endsWith('/peers')).length,1);
});

test('wall-clock sleep fences packets even if performance.now pauses', async t => {
  const h=setup(t),room=await h.start();await h.advance();
  const pc=h.connections[0];h.open(pc);
  const wake=Date.now()+30000;t.mock.method(Date,'now',()=>wake);
  assert.equal(room.active,false);assert.equal(room.send('friend',{tick:1}),false);
  pc.dc.onmessage({data:'{"afterSleep":true}'});assert.deepEqual(h.messages,[]);
  await h.advance(500);assert.equal(h.endings.length,1);
});

test('failed peer link reconnects within the same room and ignores obsolete channel events', async t => {
  const h=setup(t),room=await h.start(); await h.advance();
  const old=h.connections[0]; h.open(old);
  old.connectionState='disconnected'; old.onconnectionstatechange();
  await h.advance(5001);
  const next=h.connections[1]; assert.ok(next); assert.equal(old.closed,true); h.open(next);
  const count=h.states.length;
  old.dc.onopen(); old.dc.onmessage({data:'{"stale":true}'});
  assert.equal(h.states.length,count); assert.deepEqual(h.messages,[]);
  assert.equal(room.send('friend',{tick:4}),true); assert.equal(next.dc.sent.length,1);
  assert.equal(room.epoch,'session-one'); assert.equal(h.endings.length,0); room.close();
});

test('a data channel closing during send returns false instead of breaking the game loop', async t => {
  const h=setup(t),room=await h.start(); await h.advance();
  const pc=h.connections[0]; h.open(pc);
  pc.dc.send=()=>{throw new DOMException('Channel is closing','InvalidStateError');};
  assert.equal(room.send('friend',{tick:1}),false); room.close();
});

}
