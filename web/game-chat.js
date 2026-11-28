// Runs on the Library origin. Only the readiness signal crosses into the game.
const params=new URLSearchParams(location.hash.slice(1));
const launch=params.get('launch'), profile=params.get('profile');
const toasts=document.getElementById('toasts');
const lifetimes=new Map();
let muted=null, denied=false, cursor=null, pollTimer, request;
let fastPollUntil=0;

function dismiss(toast){
  clearTimeout(lifetimes.get(toast));
  lifetimes.delete(toast);
  toast.remove();
}
function show({author,text}){
  const toast=document.createElement('article'), name=document.createElement('strong'), message=document.createElement('p');
  name.textContent=author;message.textContent=text;
  toast.append(name,message);
  if(toasts.childElementCount===3)dismiss(toasts.firstElementChild);
  toasts.append(toast);
  lifetimes.set(toast,setTimeout(()=>dismiss(toast),6000));
}
function pause(){
  clearTimeout(pollTimer);
  request?.abort();request=null;cursor=null;
  for(const toast of lifetimes.keys())dismiss(toast);
}
async function poll(){
  if(muted!==false||document.hidden||denied||request)return;
  clearTimeout(pollTimer);
  const active=new AbortController();
  request=active;
  const url=new URL('/api/game-chat/'+encodeURIComponent(launch),location.origin);
  if(cursor!==null)url.searchParams.set('after',cursor);
  try{
    const response=await fetch(url,{headers:{'X-Infuser-Profile':profile},signal:AbortSignal.any([active.signal,AbortSignal.timeout(8000)])});
    if(request!==active)return;
    if([401,403,404,410].includes(response.status)){
      denied=true;pause();return;
    }
    if(!response.ok)throw Error();
    const data=await response.json();
    if(request!==active)return;
    if(cursor!==null && data.messages.length){
      fastPollUntil=performance.now()+30000;
      for(const message of data.messages)show(message);
    }
    cursor=data.cursor;
  }catch{
    // Reconnect with a fresh baseline instead of replaying missed conversation.
    if(request===active)cursor=null;
  }finally{
    if(request===active){request=null;pollTimer=setTimeout(poll,performance.now()<fastPollUntil?3000:10000);}
  }
}
if(parent!==window && launch && profile){
  window.addEventListener('message',event=>{
    if(event.source!==parent||event.data?.type!=='infuser-chat-mute'||typeof event.data.muted!=='boolean')return;
    // Initial unmuted setup starts idle. Only an explicit unmute speeds up polling.
    if(muted===true && !event.data.muted)fastPollUntil=performance.now()+30000;
    muted=event.data.muted;
    if(muted)pause();else void poll();
  });
  document.addEventListener('visibilitychange',()=>{if(document.hidden)pause();else void poll();});
  window.addEventListener('pagehide',pause);
  window.addEventListener('pageshow',event=>{if(event.persisted)void poll();});
  navigator.serviceWorker?.addEventListener('message',event=>{
    if(event.data?.type==='infuser-reading-chat')
      event.ports[0]?.postMessage(!denied&&!document.hidden&&event.data.owner===profile);
  });
  parent.postMessage('infuser-chat-ready','*');
}
