import {connectInfuser} from './infuser-optional.js';
const status=document.querySelector('#status'), canvas=document.querySelector('canvas'), paint=canvas.getContext('2d');
let platform, lease, socket, id, state={players:[]}, input;
async function leave() {
  clearInterval(input);const oldSocket=socket;socket=null;state={players:[]};
  if(oldSocket){oldSocket.onclose=null;oldSocket.close();}
  const oldLease=lease;lease=null;document.querySelector('#leave').hidden=true;
  document.querySelector('#join').disabled=false;document.querySelector('#own').disabled=false;
  if(oldLease && platform) await platform.runtime.release(oldLease.session_id);
}
async function play(own) {
  document.querySelector('#join').disabled=true;document.querySelector('#own').disabled=true;
  try {
    platform=await connectInfuser();
    let endpoint=new URL(location.href);endpoint.protocol=location.protocol==='https:'?'wss:':'ws:';endpoint.pathname='/';
    if(platform) {
      status.textContent='Starting server…';lease=await platform.runtime.acquire();
      const deadline=Date.now()+90_000;
      while(lease.state!=='ready') {
        if(Date.now()>deadline || lease.state==='failed') throw Error('Server could not start. Try again.');
        await new Promise(resolve=>setTimeout(resolve,500));lease=await platform.runtime.status();
      }
      endpoint=new URL(lease.endpoint.url);endpoint.protocol='wss:';endpoint.pathname=endpoint.pathname.replace(/\/$/,'')+'/';
    }
    socket=new WebSocket(endpoint);
    socket.onopen=()=>socket.send(JSON.stringify({type:'join',room:own?'own-'+crypto.randomUUID():'shared'}));
    socket.onmessage=event=>{
      const message=JSON.parse(event.data);
      if(message.type==='joined') {id=message.id;status.textContent=own?'Your run':'Shared run';document.querySelector('#leave').hidden=false;}
      if(message.type==='state') {state=message;status.textContent=`${state.players.length} playing`;}
      if(message.type==='error') status.textContent=message.message;
    };
    socket.onclose=()=>{status.textContent='Connection ended. Choose a run to reconnect.';leave().catch(error=>status.textContent=error.message);};
    input=setInterval(()=>{if(socket?.readyState===WebSocket.OPEN) socket.send(JSON.stringify({type:'input',direction:performance.now()/2300}));},100);
  } catch(error) {await leave();status.textContent=error.message;}
}
document.querySelector('#join').onclick=()=>play(false);
document.querySelector('#own').onclick=()=>play(true);
document.querySelector('#leave').onclick=async()=>{await leave();status.textContent='Choose a run';};
function draw(){paint.clearRect(0,0,720,480);for(const player of state.players){paint.fillStyle=player.id===id?'#8fffd2':'#d5b4ff';paint.beginPath();paint.arc(360+player.x*60,240+player.z*45,14,0,Math.PI*2);paint.fill();}requestAnimationFrame(draw);}draw();
if(new URLSearchParams(location.search).get('autoplay')==='1') play(false);
