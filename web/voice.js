// Voice belongs to this page's authenticated profile/context, never to a game.
const icon='<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M4 14v-3a8 8 0 0 1 16 0v3M4 12H3v8h4v-8H4zm16 0h1v8h-4v-8h3z"/></svg>';
export function initVoice({me,api,notice}) {
  if(me.inspecting)return;
  const button=document.createElement('button');button.id='voice-toggle';button.type='button';button.hidden=true;button.innerHTML=icon;
  button.setAttribute('aria-label','Voice chat off');button.setAttribute('aria-pressed','false');
  document.getElementById('chat-toggle').before(button);
  const audio=document.createElement('div');audio.hidden=true;document.body.append(audio);
  let snapshot,room,lease,track,phase='off',generation=0,polling=false,heartbeatAt=0,alive=true;
  const request=(path,options={})=>api(path,{...options,signal:AbortSignal.timeout(8000)});
  function paint(){
    const on=phase!=='off'||!!snapshot?.self;
    button.dataset.state=phase==='off'&&snapshot?.self?'elsewhere':phase;
    button.setAttribute('aria-pressed',String(on));
    const label=phase==='connecting'?'Voice chat connecting — turn off':phase==='reconnecting'?'Voice chat reconnecting — turn off':on?'Voice chat on — turn off':'Voice chat off — turn on';
    button.title=label;button.setAttribute('aria-label',label);
    button.disabled=!on&&snapshot?.available===false;
    const ids=new Set((snapshot?.participants||[]).map(p=>p.id));
    for(const person of document.querySelectorAll('.presence-person[data-player-id]')){
      const target=person.querySelector('summary');if(!target)continue;
      const badge=target.querySelector('.voice-badge');
      if(ids.has(person.dataset.playerId)){
        if(!badge){const mark=document.createElement('span');mark.className='voice-badge';mark.innerHTML=icon;mark.title='In voice chat';mark.setAttribute('role','img');mark.setAttribute('aria-label','In voice chat');target.append(mark);}
      }else badge?.remove();
    }
  }
  const observer=new MutationObserver(paint);observer.observe(document.getElementById('friends'),{childList:true,subtree:true});
  async function stop({remote=true,message}={}){
    generation++;const old=lease||snapshot?.self?.id;lease=null;
    const current=room;room=null;phase='off';track?.stop();track=null;
    if(snapshot){snapshot.self=null;snapshot.participants=snapshot.participants.filter(p=>p.id!==me.player_id);}
    audio.replaceChildren();paint();
    if(current)void current.disconnect();
    if(remote&&old){try{await request('/api/voice',{method:'DELETE',body:JSON.stringify({id:old}),keepalive:true});}catch{/* The server's expiring lease and reconciler also remove abandoned connections. */}}
    if(message)notice(message);
  }
  async function start(){
    const attempt=++generation;phase='connecting';paint();
    try {
      if(!navigator.mediaDevices?.getUserMedia)throw Error('This browser cannot use a microphone here.');
      const {Room,RoomEvent,createLocalAudioTrack}=await import('/assets/vendor/livekit-2.24.1.js');
      if(attempt!==generation)return;
      const mic=await createLocalAudioTrack({echoCancellation:true,noiseSuppression:true,autoGainControl:true});
      if(attempt!==generation){mic.stop();return;}track=mic;
      const id=crypto.randomUUID();lease=id;
      const grant=await request('/api/voice',{method:'POST',body:JSON.stringify({id})});
      if(attempt!==generation){await request('/api/voice',{method:'DELETE',body:JSON.stringify({id})});return;}
      lease=grant.id;
      const current=new Room({singlePeerConnection:false,disconnectOnPageLeave:true});room=current;
      current.on(RoomEvent.TrackSubscribed,t=>{if(t.kind==='audio'){const element=t.attach();element.autoplay=true;audio.append(element);}});
      current.on(RoomEvent.TrackUnsubscribed,t=>t.detach().forEach(el=>el.remove()));
      current.on(RoomEvent.Reconnecting,()=>{if(room===current){phase='reconnecting';paint();}});
      current.on(RoomEvent.Reconnected,()=>{if(room===current){phase='on';paint();}});
      current.on(RoomEvent.Disconnected,()=>{if(room===current)void stop({message:'Voice chat disconnected. Tap the headset to join again.'});});
      await current.connect(grant.url,grant.token,{autoSubscribe:true,maxRetries:1});
      if(attempt!==generation){void current.disconnect();return;}
      await current.localParticipant.publishTrack(mic);
      await current.startAudio();
      if(attempt!==generation)return;
      phase='on';heartbeatAt=0;paint();void poll();
    }catch(error){
      if(attempt!==generation)return;
      const message=error.name==='NotAllowedError'?'Allow microphone access in your browser to use voice chat.':error.name==='NotFoundError'?'Connect a microphone to use voice chat.':error.message||'Could not connect to voice chat.';
      await stop({message});
    }
  }
  button.onclick=()=>{if(phase!=='off'||snapshot?.self)void stop();else void start();};
  async function poll(){
    if(polling||!alive)return;polling=true;
    try{
      if(lease&&room&&Date.now()-heartbeatAt>20000){
        await request('/api/voice/heartbeat',{method:'POST',body:JSON.stringify({id:lease})});heartbeatAt=Date.now();
      }
      const status=await request('/api/voice');snapshot=status;button.hidden=!status.configured;
      if(!status.configured)clearInterval(timer);
      if(room&&phase!=='connecting')phase=room.state==='connected'&&status.available?'on':'reconnecting';
      if(lease&&phase!=='connecting'&&status.self?.id!==lease){await stop({remote:false,message:'Voice chat was turned off.'});}
      paint();
    }catch(error){
      if(lease&&[401,403].includes(error.status))await stop({message:'Voice chat ended. Your access changed.'});
      else if(room){phase='reconnecting';paint();}
    }finally{polling=false;}
  }
  // Page navigation (including profile/visit changes) stops capture immediately.
  // A pinned request retains the old profile headers while navigation proceeds.
  addEventListener('pagehide',()=>{alive=false;observer.disconnect();clearInterval(timer);void stop({remote:!!lease});});
  const timer=setInterval(()=>{if(lease||!document.hidden)void poll();},3000);
  document.addEventListener('visibilitychange',()=>{if(!document.hidden)void poll();});
  void poll();
  return {stop};
}
