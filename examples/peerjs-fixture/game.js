import {connectInfuser} from './infuser-platform.js';
const params=new URLSearchParams(location.search), status=document.querySelector('#status');
const test=window.peerjsTest={received:0,opens:0,binary:0,role:'solo',session:null,platform:null,ended:null};
let platform, room, standalone, connections=[], remote=null;
const canvas=document.querySelector('canvas'), ctx=canvas.getContext('2d');
function handleConnection(connection) {
  connections.push(connection);
  connection.on('open',()=>test.opens++);
  connection.on('data',data=>{
    test.received++;
    if(data instanceof Uint8Array || data instanceof ArrayBuffer){test.binary++;return;}
    if(data?.type==='position') remote=data;
  });
  connection.on('close',()=>{connections=connections.filter(c=>c!==connection);});
}
async function multiplayer() {
  if(room?.active || standalone)return;
  platform=await connectInfuser();test.platform=platform?'infuser':'standalone';
  if(platform) {
    const intent=await platform.launchIntent.consume();
    const target=intent.intent;
    room=await platform.peerjs.join({Peer:window.Peer,relayOnly:params.has('relay'),joinable:true,
      sessionId:target?.session_id,playerId:target?.player_id,
      connectionOptions:{serialization:'binary',reliable:true},onConnection:handleConnection,
      onState:s=>{status.textContent=s.connected.length?'Connected':'Waiting for a friend';},
      onEnd:reason=>{test.ended=reason;status.textContent='Session ended';}});
    test.role=room.role;test.session=room.epoch;test.room=room;
  } else {
    // A conventional PeerJS path, useful for local development without Infuser.
    // These parameters are only for this test fixture's local PeerServer.
    const id=params.get('id')||crypto.randomUUID();
    standalone=new Peer(id,{host:location.hostname,port:Number(params.get('peerport')||7780),path:'/',secure:false,config:{iceServers:[]}});
    standalone.on('connection',handleConnection);
    standalone.on('open',()=>{if(params.has('friend'))handleConnection(standalone.connect(params.get('friend'),{serialization:'binary',reliable:true}));});
    standalone.on('error',e=>{test.error=e.type;});
    test.role=params.has('friend')?'member':'host';
  }
}
document.querySelector('#play').onclick=()=>multiplayer().catch(e=>{test.error=e.message;status.textContent=e.message;});
let tick=0;
setInterval(()=>{
  tick++;const position={type:'position',x:400+250*Math.sin(tick/35),y:200+120*Math.cos(tick/49)};
  for(const c of connections)if(c.open){c.send(position);if(tick%10===0)c.send(new Uint8Array([1,2,3,4]));}
  ctx.clearRect(0,0,800,400);ctx.fillStyle='#7cebd2';ctx.beginPath();ctx.arc(position.x,position.y,12,0,7);ctx.fill();
  if(remote){ctx.fillStyle='#ffbc7c';ctx.beginPath();ctx.arc(remote.x,remote.y,12,0,7);ctx.fill();}
},50);
if(params.has('autoplay'))multiplayer().catch(e=>{test.error=e.message;status.textContent=e.message;});
