import test from 'node:test';
import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {setImmediate} from 'node:timers/promises';
import {join} from '../sdk/v1/peerjs.js';
import {connectInfuser} from '../sdk/optional.js';
function fixture(t) {
  let clock=1000,poll,renewalFailures=0;
  const peers=[],messages=[],ended=[],requests=[];
  const snapshot={self_id:'a'.repeat(32),host_id:'b'.repeat(32),epoch:'c'.repeat(32),role:'member',lease_ms:25000,
    peers:[{id:'a'.repeat(32)},{id:'b'.repeat(32)}],ice_servers:[{urls:['turn:example.com:3478'],expires_at:Math.floor(Date.now()/1000)+3600}],ice_transport_policy:'relay',peerjs:{path:'/_infuser/peerjs/',key:'peerjs'}};
  t.mock.timers.enable({apis:['setTimeout','setInterval']});t.mock.method(performance,'now',()=>clock);
  function install(key,value){const descriptor=Object.getOwnPropertyDescriptor(globalThis,key);Object.defineProperty(globalThis,key,{configurable:true,writable:true,value});t.after(()=>descriptor?Object.defineProperty(globalThis,key,descriptor):delete globalThis[key]);}
  install('window',{parent:{postMessage(){}}});install('location',{hostname:'game.example.com',port:'',protocol:'https:',origin:'https://game.example.com'});
  install('fetch',async(path,options)=>{
    requests.push({path,method:options.method,body:options.body&&JSON.parse(options.body)});
    if(path.endsWith('/ice')){if(renewalFailures-->0)return Response.json({error:'temporary'},{status:503});return Response.json({ice_servers:[{urls:['turn:example.com:3478'],credential:'renewed',expires_at:Date.now()/1000+3600}]});}
    if(options.method==='GET'&&poll){const pending=poll;poll=null;return pending;}
    return Response.json(snapshot);
  });
  class Connection extends EventEmitter {
    constructor(id){super();this.peer=id;this.open=true;this.dataChannel={bufferedAmount:0};this.sent=[];this.peerConnection={connectionState:'connected',setConfiguration:c=>{this.config=c;},getStats:async()=>new Map()};}
    close(){this.open=false;this.emit('close');}
    send(data){this.sent.push(data);}
  }
  class Peer extends EventEmitter {
    constructor(id,options){super();this.id=id;this.options=options;this.open=true;peers.push(this);}
    connect(id){return new Connection(id);}
    destroy(){this.destroyed=true;}
    reconnect(){this.disconnected=false;this.reconnections=(this.reconnections||0)+1;}
  }
  return {snapshot,peers,messages,ended,requests,
    start:()=>join({Peer,onConnection:c=>c.on('data',d=>messages.push(d)),onEnd:e=>ended.push(e)}),
    poll:value=>poll=value,failRenewal:()=>renewalFailures=1,
    jump:ms=>clock+=ms,
    async advance(ms=0){clock+=ms;t.mock.timers.tick(ms);await setImmediate();}
  };
}
test('PeerJS sends preserve binary data and fence stale callbacks before sleep timers run',async t=>{
  const h=fixture(t),room=await h.start();await h.advance();
  const c=room.connections[0],bytes=new Uint8Array([0,255]);
  assert.equal(c.send(bytes),true);assert.equal(c.sent[0],bytes);
  c.emit('data',bytes);assert.equal(h.messages[0],bytes);
  h.jump(25001);
  assert.equal(c.send(bytes),false);c.emit('data','stale');assert.equal(h.messages.length,1);
  assert.equal(room.active,false);await h.advance(500);assert.equal(h.ended.length,1);assert.equal(h.peers[0].destroyed,true);
});
test('PeerJS fences sleep when only the wall clock advances',async t=>{
  const h=fixture(t),room=await h.start();await h.advance();
  const connection=room.connections[0],wake=Date.now()+30000;
  t.mock.method(Date,'now',()=>wake);
  assert.equal(connection.send('late'),false);connection.emit('data','late');
  assert.deepEqual(h.messages,[]);assert.equal(room.active,false);
  await h.advance(500);assert.equal(h.ended.length,1);
});
test('PeerJS transient poll and renewal failures recover without creating another shared room',async t=>{
  const h=fixture(t);h.snapshot.ice_servers[0].expires_at=Date.now()/1000+100;h.failRenewal();
  const room=await h.start();
  h.poll(Response.json({error:'temporary'},{status:503}));await h.advance();assert(room.active);
  await h.advance(1000);await h.advance(1000);
  assert.equal(h.peers[0].options.config.iceServers[0].credential,'renewed');
  assert.equal(h.requests.filter(r=>r.path.endsWith('/ice')).length,2);
  assert.equal(h.requests.filter(r=>r.path.endsWith('/peers')&&r.method==='POST').length,1);
  assert.equal(h.requests[0].body.adapter,'peerjs');
  assert.equal(h.peers[0].options.config.iceTransportPolicy,'relay');
  room.close();
});
test('PeerJS reconnects signaling, and late poll replies cannot resurrect a closed room',async t=>{
  const h=fixture(t),room=await h.start();await h.advance();
  h.peers[0].disconnected=true;await h.advance(1000);assert.equal(h.peers[0].reconnections,1);
  let resolve;h.poll(new Promise(r=>resolve=r));await h.advance(1000);
  const connection=room.connections[0];room.close();resolve(Response.json(h.snapshot));await setImmediate();
  assert(!room.active);assert.equal(h.ended.length,1);assert.equal(room.connections.length,0);
  connection.emit('data','late');assert.deepEqual(h.messages,[]);
});
test('standalone bootstrap makes no platform requests',async t=>{
  const original=globalThis.document;globalThis.document={scripts:[]};t.after(()=>{if(original===undefined)delete globalThis.document;else globalThis.document=original;});
  t.mock.method(globalThis,'fetch',()=>{throw new Error('unexpected platform dependency');});
  assert.equal(await connectInfuser(),null);
});
