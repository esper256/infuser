const status=document.getElementById('status'),button=document.getElementById('approve'),signin=document.getElementById('signin');
const requestId=new URLSearchParams(location.search).get('pair');
signin.href='/oauth2/start?rd='+encodeURIComponent(location.pathname+location.search);
async function start(){
 if(!/^[a-f0-9]{64}$/.test(requestId||'')){status.textContent='Open a game from your Library to connect Infuser.';return;}
 const response=await fetch('/api/me');
 if(!response.ok){status.textContent='Sign in to connect this computer.';signin.hidden=false;return;}
 const me=await response.json();status.textContent='Connect as '+me.name+'?';button.hidden=false;
}
button.onclick=async()=>{
 button.disabled=true;
 try{const r=await fetch('/api/native/v1/device/approve',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({request_id:requestId})});const result=await r.json();if(!r.ok)throw Error(result.error||'Could not connect.');status.textContent='Connected! Infuser will open your game.';button.hidden=true;document.getElementById('hint').hidden=true;}
 catch(e){status.textContent=e.message;button.disabled=false;}
};
start().catch(()=>{status.textContent='Could not reach Infuser. Refresh to try again.';});
