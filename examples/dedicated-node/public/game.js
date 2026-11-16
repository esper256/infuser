import {connectInfuser} from './infuser-optional.js';
const status=document.querySelector('#status'), canvas=document.querySelector('canvas'), paint=canvas.getContext('2d');
let platform, lease, socket, id, state={players:[]}, input, resumeTarget;
const button=name=>document.querySelector('#'+name);
function controls(disabled){for(const name of ['join','own','new','resume'])button(name).disabled=disabled;}
async function leave() {
  clearInterval(input);const old=socket;socket=null;state={players:[]};
  if(old){old.onclose=null;if(platform && old.infuser)await platform.sessions.disconnect(old).catch(()=>{});else old.close();}
  const oldLease=lease;lease=null;button('leave').hidden=true;controls(false);
  if(oldLease && platform)await platform.runtime.release(oldLease.session_id);
}
async function play(options={}) {
  controls(true);
  try {
    platform=await connectInfuser();
    if(platform) {
      status.textContent='Connecting…';socket=await platform.sessions.connect(options);
      lease={session_id:socket.infuser.lease_id};resumeTarget={session_id:socket.infuser.session.id};
    } else {
      const endpoint=new URL('/',location.href);endpoint.protocol=location.protocol==='https:'?'wss:':'ws:';
      socket=new WebSocket(endpoint);
      socket.onopen=()=>socket.send(JSON.stringify({type:'join',room:options.activity==='solo'||options.action==='create'?'own-'+crypto.randomUUID():'shared'}));
    }
    socket.onmessage=event=>{
      const message=JSON.parse(event.data);
      if(message.type==='joined'){id=message.id;status.textContent=message.name?`Playing as ${message.name}`:'Connected';button('leave').hidden=false;button('resume').hidden=true;}
      if(message.type==='state')state=message;
      if(message.type==='error')status.textContent=message.message;
    };
    socket.onclose=()=>{clearInterval(input);socket=null;controls(false);button('leave').hidden=true;button('resume').hidden=!resumeTarget;status.textContent='Disconnected. Resume this run or choose another.';};
    input=setInterval(()=>{if(socket?.readyState===WebSocket.OPEN)socket.send(JSON.stringify({type:'input',direction:performance.now()/2300}));},100);
  } catch(error){await leave();status.textContent=error.message;}
}
button('join').onclick=()=>play({activity:'multiplayer'});
button('own').onclick=()=>play({activity:'solo'});
button('new').onclick=()=>play({action:'create',activity:'multiplayer'});
button('resume').onclick=()=>play({action:'resume',target:resumeTarget});
button('leave').onclick=async()=>{await leave();resumeTarget=null;button('resume').hidden=true;status.textContent='Choose a run';};
function draw(){paint.clearRect(0,0,720,480);for(const player of state.players){paint.fillStyle=player.id===id?'#8fffd2':'#d5b4ff';paint.beginPath();paint.arc(360+player.x*60,240+player.z*45,14,0,Math.PI*2);paint.fill();}requestAnimationFrame(draw);}draw();
async function friends(){
  if(!platform)return;
  try {
    const presence=await platform.presence.get();const list=button('friends');list.replaceChildren();
    for(const session of presence.sessions||[])for(const friend of session.players||[]) {
      if(!session.joinable||friend.player_id===presence.self_id)continue;
      const join=document.createElement('button');join.textContent='Join '+friend.name;
      join.onclick=()=>play({action:'join',target:{session_id:session.id,player_id:friend.player_id}});list.append(join);
    }
  }catch{}
}
platform=await connectInfuser();
const intent=platform?await platform.launchIntent.consume():null;
if(intent?.intent)play({intent:intent.intent});
else if(new URLSearchParams(location.search).get('autoplay')==='1')play({activity:'multiplayer'});
setInterval(friends,3000);friends();
// Fixture automation only; no credentials or Google access are exposed here.
window.fixture={play,leave,get socket(){return socket},get state(){return state},get id(){return id},get platform(){return platform}};
