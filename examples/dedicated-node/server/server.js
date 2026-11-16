import http from 'node:http';
import {readFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';
import {Vector3} from 'three';
import {WebSocketServer, WebSocket} from 'ws';

// A small authoritative simulation: several rooms in one server process.
// Room rules and gameplay messages belong to this game, not to Infuser.
const rooms = new Map();
const boot = randomUUID();
let tick = 0;
const server = http.createServer(async (req,res) => {
  if (req.url === '/healthz') {
    res.writeHead(200, {'content-type':'application/json'});
    res.end(JSON.stringify({ok:true,boot,tick,rooms:rooms.size}));
    return;
  }
  // Standalone convenience. Infuser serves the public export separately.
  const page = {'/':'index.html','/game.js':'game.js','/infuser-optional.js':'infuser-optional.js'}[req.url];
  if (page) {
    try {
      const bytes = await readFile(new URL('../public/'+page, import.meta.url));
      res.writeHead(200, {'content-type':page.endsWith('.js')?'text/javascript':'text/html'});
      res.end(bytes); return;
    } catch {}
  }
  res.writeHead(404); res.end();
});
const sockets = new WebSocketServer({server,maxPayload:64*1024});
function send(socket,value) {if(socket.readyState===WebSocket.OPEN && socket.bufferedAmount<128*1024) socket.send(JSON.stringify(value));}
sockets.on('connection', socket => {
  if (sockets.clients.size > 64) {socket.close(1013,'Server full'); return;}
  const id = randomUUID();
  let room;
  const joinTimeout = setTimeout(()=>socket.close(1008,'Choose a room'),10000);
  socket.on('message', bytes => {
    try {
      const message=JSON.parse(bytes.toString());
      if(message.type==='join' && !room) {
        const name=message.room;
        if(typeof name!=='string'||!/^[a-zA-Z0-9_-]{1,48}$/.test(name)) throw Error('Invalid room');
        if(!rooms.has(name)) {
          if(rooms.size>=30) throw Error('No room capacity');
          rooms.set(name,new Map());
        }
        room=rooms.get(name);
        if(room.size>=8) {room=undefined; throw Error('Room full');}
        room.set(id,{socket,position:new Vector3(),direction:0});
        clearTimeout(joinTimeout);
        send(socket,{type:'joined',id,room:name,boot});
      } else if(message.type==='input' && room) {
        if(!Number.isFinite(message.direction)) throw Error('Invalid input');
        room.get(id).direction=message.direction% (Math.PI*2);
      }
    } catch(error) {send(socket,{type:'error',message:error.message});}
  });
  socket.on('close',()=>{
    clearTimeout(joinTimeout);
    if(!room) return;
    room.delete(id);
    for(const [name,players] of rooms) if(players.size===0) rooms.delete(name);
  });
});
const timer=setInterval(()=>{
  tick++;
  for(const room of rooms.values()) {
    for(const player of room.values()) {
      player.position.add(new Vector3(Math.cos(player.direction),0,Math.sin(player.direction)).multiplyScalar(0.03));
      if(player.position.length()>4) player.position.multiplyScalar(-0.95);
    }
    if(tick%2===0) {
      const snapshot={type:'state',tick,players:[...room].map(([id,p])=>({id,x:p.position.x,z:p.position.z}))};
      for(const p of room.values()) send(p.socket,snapshot);
    }
  }
},1000/30);
server.listen(Number(process.env.PORT||8080),'0.0.0.0');
process.on('SIGTERM',()=>{
  clearInterval(timer);
  for(const socket of sockets.clients) socket.close(1001,'Server stopping');
  server.close(()=>process.exit(0));
  setTimeout(()=>process.exit(0),3000).unref();
});
