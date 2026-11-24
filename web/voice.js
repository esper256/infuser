// Voice belongs to this page's authenticated profile/context, never to a game.
const icon='<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M4 13v-2a8 8 0 0 1 16 0v2M4 12H3v7h4v-7H4zm16 0h1v6h-4v-6h3zM20 18v1a3 3 0 0 1-3 3h-3"/><rect x="10" y="20" width="5" height="3" rx="1.5" fill="currentColor" stroke="none"/></svg>';
const deafenIcon='<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M4 13v-2a8 8 0 0 1 16 0v2M4 12H3v7h4v-7H4zm16 0h1v6h-4v-6h3zM3 3l18 18"/></svg>';
const micOnIcon='<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><rect x="9" y="2" width="6" height="11" rx="3"/><path d="M5 10a7 7 0 0 0 14 0M12 17v4M8 21h8"/></svg>';
const micOffIcon='<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M9 5a3 3 0 0 1 6 0v5M5 10a7 7 0 0 0 11.9 5M12 17v4M8 21h8M3 3l18 18"/></svg>';
const slidersIcon='<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M4 6h16M4 12h16M4 18h16"/><circle cx="9" cy="6" r="2.2" fill="currentColor"/><circle cx="15" cy="12" r="2.2" fill="currentColor"/><circle cx="10" cy="18" r="2.2" fill="currentColor"/></svg>';
const speakerOnIcon='<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M11 5L6 9H2v6h4l5 4V5zM15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14"/></svg>';
const speakerOffIcon='<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M11 5L6 9H2v6h4l5 4V5zM23 9l-6 6M17 9l6 6"/></svg>';

// Local output only: never attach these tones to the microphone or publish them.
export function createVoiceChimes(){
  let context,closing,last=-Infinity,quietUntil=0,volumeScale=1;
  return {
    unlock(){
      clearTimeout(closing);
      try{
        const Audio=globalThis.AudioContext||globalThis.webkitAudioContext;
        if(!context||context.state==='closed')context=new Audio();
        void context.resume().catch(()=>{});
      }catch{/* Chimes are optional; audio restrictions must not prevent voice. */}
    },
    setVolumeScale(scale){
      const next=Number(scale);
      volumeScale=Number.isFinite(next)?Math.max(0,Math.min(1,next)):1;
    },
    quiet(){quietUntil=performance.now()+2000;},
    play(joined){
      const now=performance.now();
      // One shared cooldown, with no delayed queue to replay stale events.
      if(!context||context.state!=='running'||volumeScale<=0||now<quietUntil||now-last<4000)return;
      last=now;
      try{
        const peak=.035*volumeScale;
        const notes=joined?[440,554.37,659.25]:[659.25,554.37,440];
        notes.forEach((frequency,index)=>{
          const start=context.currentTime+index*.13,oscillator=context.createOscillator(),gain=context.createGain();
          oscillator.type='sine';oscillator.frequency.value=frequency;
          gain.gain.setValueAtTime(0,start);
          gain.gain.linearRampToValueAtTime(peak,start+.025);
          gain.gain.exponentialRampToValueAtTime(.0001,start+.38);
          gain.gain.linearRampToValueAtTime(0,start+.42);
          oscillator.connect(gain);gain.connect(context.destination);
          oscillator.onended=()=>{oscillator.disconnect();gain.disconnect();};
          oscillator.start(start);oscillator.stop(start+.43);
        });
      }catch{/* Device suspension or loss must not affect the conversation. */}
    },
    close(immediate=false){
      clearTimeout(closing);
      const old=context;
      const finish=()=>{if(context===old)context=null;if(old&&old.state!=='closed')void old.close().catch(()=>{});};
      if(immediate)finish();else closing=setTimeout(finish,750);
    },
  };
}

const DEVICE_STORAGE_KEY='infuser.voice.devices';
function readDevices(){
  try{const parsed=JSON.parse(localStorage.getItem(DEVICE_STORAGE_KEY)||'{}');return {inputId:typeof parsed.inputId==='string'?parsed.inputId:'',outputId:typeof parsed.outputId==='string'?parsed.outputId:''};}
  catch{return {inputId:'',outputId:''};}
}
function saveDevices(devices){
  try{localStorage.setItem(DEVICE_STORAGE_KEY,JSON.stringify(devices));}catch{/* Ignore storage quota errors. */}
}

export function initVoice({me,api,notice,diagnostics,onConfiguredChange=()=>{}}) {
  if(me.inspecting)return {stop:async()=>{},openSettings:()=>{},renderFriendControls:()=>{}};

  const splitSlot=document.createElement('span');
  splitSlot.id='voice-split-slot';splitSlot.hidden=true;
  document.getElementById('chat-toggle').before(splitSlot);

  const mixer=document.createElement('section');
  mixer.id='voice-mixer';mixer.className='voice-mixer';mixer.setAttribute('popover','auto');
  mixer.setAttribute('role','dialog');mixer.setAttribute('aria-label','Voice levels and settings');
  document.body.append(mixer);

  const audio=document.createElement('div');audio.hidden=true;document.body.append(audio);

  const audioModPromise=import('/assets/voice-audio.js');
  const splitModPromise=import('/assets/voice-split.js');
  let audioMod=null,split=null;
  let snapshot=null,room=null,lease=null,track=null,phase='off',generation=0,polling=false,heartbeatAt=0,alive=true;
  let settings={input_gain:100,output_volume:100,auto_gain_control:true,noise_suppression:true,echo_cancellation:true,peer_volumes:{}};
  let serverEpoch='',revision=0,lastAckedKey='',micMuted=false,deafened=false,audioApplySeq=0;
  let voiceCtx=null,inputProcessor=null,outputController=null;
  let identityToPlayer=new Map(),speakingIds=new Set();
  let controlAbort=null,controlRunning=false;
  let pendingPatch=null,patchTimer=null,sendingPatch=false,dragging=false;
  let mixerAnchor=null,lastMixerSig='',renderedDevicesSig='',meterFrame=0,lastMeterAt=0,meterFill=null;
  let testStream=null,testProcessor=null,testingMic=false;
  let devices=readDevices(),deviceList={inputs:[],outputs:[]},optionsOpen=false;
  let wasConfigured=false;

  async function ensureSplit(){
    if(split)return split;
    const mod=await splitModPromise;
    if(split)return split;
    split=mod.createVoiceSplit({
      withToggle:true,
      controlsId:'voice-mixer',
      onToggle:()=>{if(phase!=='off'||snapshot?.self)void stop({reason:'user_toggle'});else void start();},
      onOpen:()=>openSettings(split.caretBtn),
      onClose:()=>{if(mixer.matches(':popover-open'))mixer.hidePopover();},
    });
    splitSlot.replaceWith(split.root);
    mixerAnchor=split.caretBtn;
    return split;
  }

  const chimes=createVoiceChimes();
  const request=(path,options={})=>api(path,{...options,signal:options.signal||AbortSignal.timeout(8000)});

  function clampPct(v){return audioMod?audioMod.clampPercent(v):Math.max(0,Math.min(200,Math.round(Number(v)||0)));}
  function pctToGain(v){return audioMod?audioMod.percentToGain(v):(v<=0?0:v<=100?(v/100)**2:1+((v-100)/100)*1.5);}
  function ensureVoiceContext(){
    try{
      const Audio=globalThis.AudioContext||globalThis.webkitAudioContext;
      if(!Audio)return null;
      if(!voiceCtx||voiceCtx.state==='closed')voiceCtx=new Audio();
      if(voiceCtx.state==='suspended')void voiceCtx.resume().catch(()=>{});
      return voiceCtx;
    }catch{return null;}
  }
  function closeVoiceContextIfIdle(){
    if(room||testingMic)return;
    const old=voiceCtx;voiceCtx=null;
    if(old&&old.state!=='closed')void old.close().catch(()=>{});
  }

  function peerPref(playerId){
    const raw=settings.peer_volumes?.[playerId];
    return {volume:clampPct(raw?.volume??100),muted:Boolean(raw?.muted)};
  }

  function gainForIdentity(identity){
    if(deafened)return 0;
    const playerId=identityToPlayer.get(identity);
    const pref=playerId?peerPref(playerId):{volume:100,muted:false};
    if(pref.muted)return 0;
    return pctToGain(settings.output_volume)*pctToGain(pref.volume);
  }

  async function applyAudioState({ack=false}={}){
    const seq=++audioApplySeq;
    const targetRev=revision,targetEpoch=serverEpoch,targetLease=lease;
    const targetMicMuted=Boolean(micMuted),targetDeafened=Boolean(deafened);
    chimes.setVolumeScale(targetDeafened?0:Math.min(1,pctToGain(settings.output_volume)));
    const inputLinear=pctToGain(settings.input_gain);
    inputProcessor?.setGain(inputLinear);
    testProcessor?.setGain(inputLinear);
    outputController?.applyAll(identity=>gainForIdentity(identity));
    let muteOk=true;
    if(track){
      const shouldMute=Boolean(targetMicMuted||targetDeafened);
      try{
        if(shouldMute&&!track.isMuted)await track.mute();
        else if(!shouldMute&&track.isMuted)await track.unmute();
        if(Boolean(track.isMuted)!==shouldMute)muteOk=false;
      }catch{
        muteOk=false;
      }
    }
    if(!alive||seq!==audioApplySeq)return false;
    if(!muteOk){
      if(track){
        const actualMuted=Boolean(track.isMuted);
        if(!actualMuted){micMuted=false;deafened=false;}
        else{micMuted=true;}
        paint();
        if(mixer.matches(':popover-open')&&!dragging)renderMixer();
        if(targetLease&&targetLease===lease&&phase!=='off'){
          try{
            const reconciled=await request('/api/voice/settings',{
              method:'PATCH',
              body:JSON.stringify({mic_muted:micMuted,deafened}),
            });
            if(reconciled?.epoch)serverEpoch=reconciled.epoch;
            if(typeof reconciled?.revision==='number')revision=Math.max(revision,reconciled.revision);
            ackApplied({id:targetLease,epoch:serverEpoch,rev:revision,mic:micMuted,deaf:deafened});
          }catch{/* Next status poll reconciles server lease. */}
        }
      }
      notice('Could not update microphone mute state.');
      return false;
    }
    if(ack&&track&&targetLease&&targetLease===lease&&phase!=='off'){
      ackApplied({id:targetLease,epoch:targetEpoch,rev:targetRev,mic:targetMicMuted,deaf:targetDeafened});
    }
    return true;
  }

  async function applyTrackConstraints(){
    if(!track||phase!=='on')return;
    try{
      await track.restartTrack({
        echoCancellation:true,
        noiseSuppression:settings.noise_suppression,
        autoGainControl:settings.auto_gain_control,
        ...(devices.inputId?{deviceId:devices.inputId}:{}),
      });
      await applyAudioState({ack:Boolean(lease&&phase!=='off')});
    }catch{
      notice('Could not update microphone settings.');
    }
  }

  async function applyOutputDevice(){
    if(!outputController)return;
    try{await outputController.setSinkId(devices.outputId);}catch{/* Unsupported sink or device removed. */}
  }

  function syncIdentityMap(){
    identityToPlayer.clear();
    for(const person of snapshot?.participants||[]){
      if(person.participant_id&&person.id)identityToPlayer.set(person.participant_id,person.id);
    }
    if(lease&&me.player_id)identityToPlayer.set(lease,me.player_id);
  }

  function ackApplied({id=lease,epoch=serverEpoch,rev=revision,mic=micMuted,deaf=deafened}={}){
    if(!id||phase==='off'||!alive)return;
    const key=`${id}:${epoch}:${rev}:${mic}:${deaf}`;
    if(key===lastAckedKey)return;
    lastAckedKey=key;
    void request('/api/voice/ack',{method:'POST',body:JSON.stringify({id,revision:rev,mic_muted:mic,deafened:deaf})}).catch(()=>{
      if(lastAckedKey===key)lastAckedKey='';
    });
  }

  function syncServerState(data){
    if(!data)return;
    const incomingEpoch=typeof data.epoch==='string'?data.epoch:'';
    if(incomingEpoch&&incomingEpoch!==serverEpoch){
      serverEpoch=incomingEpoch;
      revision=0;
      lastAckedKey='';
    }
    const incomingRev=Number(data.revision||0);
    if(incomingRev>=revision||revision===0){
      revision=Math.max(revision,incomingRev);
      if(data.settings&&!pendingPatch&&!dragging){
        settings=audioMod?audioMod.normalizeSettings(data.settings):data.settings;
      }
      if(data.self!==undefined&&!pendingPatch){
        if(data.self){
          micMuted=Boolean(data.self.mic_muted);
          deafened=Boolean(data.self.deafened);
        }else if(phase==='off'){
          micMuted=false;deafened=false;
        }
      }
      void applyAudioState({ack:Boolean(lease&&phase!=='off')});
    }
  }

  function mergePatch(patch){
    if(!pendingPatch)pendingPatch={};
    for(const [key,value] of Object.entries(patch)){
      if(key==='peer_volumes'){
        pendingPatch.peer_volumes={...(pendingPatch.peer_volumes||{})};
        for(const [pid,entry] of Object.entries(value)){
          pendingPatch.peer_volumes[pid]={...(pendingPatch.peer_volumes[pid]||{}),...entry};
        }
      }else{
        pendingPatch[key]=value;
      }
    }
  }

  async function flushPatch(){
    clearTimeout(patchTimer);patchTimer=null;
    if(!pendingPatch||sendingPatch||!alive)return;
    const payload=pendingPatch;pendingPatch=null;sendingPatch=true;
    try{
      const updated=await request('/api/voice/settings',{method:'PATCH',body:JSON.stringify(payload)});
      syncServerState(updated);
      paint();
      if(mixer.matches(':popover-open')&&!dragging)renderMixer();
    }catch(error){
      if(error.status&&error.status>=400&&error.status<500)notice(error.message);
    }finally{
      sendingPatch=false;
      if(pendingPatch)void flushPatch();
    }
  }

  function queuePatch(patch,{immediate=false,reapplyConstraints=false}={}){
    // Optimistically update local audio & UI for zero-latency responsiveness.
    if(typeof patch.input_gain==='number')settings.input_gain=clampPct(patch.input_gain);
    if(typeof patch.output_volume==='number'){
      settings.output_volume=clampPct(patch.output_volume);
      if(settings.output_volume>100)ensureVoiceContext();
    }
    if(typeof patch.auto_gain_control==='boolean')settings.auto_gain_control=patch.auto_gain_control;
    if(typeof patch.noise_suppression==='boolean')settings.noise_suppression=patch.noise_suppression;
    if(typeof patch.deafened==='boolean')deafened=patch.deafened;
    if(typeof patch.mic_muted==='boolean'){
      micMuted=patch.mic_muted;
      if(!patch.mic_muted)deafened=false;
    }
    if(patch.peer_volumes){
      settings.peer_volumes={...(settings.peer_volumes||{})};
      for(const [pid,update] of Object.entries(patch.peer_volumes)){
        const prev=peerPref(pid);
        const next={volume:update.volume!==undefined?clampPct(update.volume):prev.volume,muted:update.muted!==undefined?Boolean(update.muted):prev.muted};
        if(next.volume>100)ensureVoiceContext();
        if(next.volume===100&&!next.muted)delete settings.peer_volumes[pid];
        else settings.peer_volumes[pid]=next;
      }
    }
    void applyAudioState();
    paint();
    if(reapplyConstraints)void applyTrackConstraints();
    mergePatch(patch);
    if(immediate)void flushPatch();
    else if(!patchTimer)patchTimer=setTimeout(()=>{void flushPatch();},100);
  }

  function paint(expandedOverride){
    const configured=Boolean(snapshot?.configured);
    if(configured!==wasConfigured){wasConfigured=configured;onConfiguredChange(configured);}
    const on=phase!=='off'||Boolean(snapshot?.self);
    const effectiveMuted=Boolean(micMuted||deafened);
    const state=phase==='off'&&snapshot?.self?'elsewhere':phase;
    split?.setState({
      visible:configured,
      on,
      live:phase==='on',
      state,
      muted:effectiveMuted,
      deafened:Boolean(on&&deafened),
      syncing:false,
      connected:phase==='on',
      disabled:!on&&snapshot?.available===false,
      expanded:typeof expandedOverride==='boolean'?expandedOverride:mixer.matches(':popover-open'),
    });

    const ids=new Set((snapshot?.participants||[]).map(p=>p.id));
    for(const person of document.querySelectorAll('.presence-person[data-player-id]')){
      const target=person.querySelector('summary');if(!target)continue;
      const badge=target.querySelector('.voice-badge');
      const pid=person.dataset.playerId;
      person.classList.toggle('voice-speaking',speakingIds.has(pid));
      if(ids.has(pid)){
        if(!badge){const mark=document.createElement('span');mark.className='voice-badge';mark.innerHTML=icon;mark.title='In voice chat';mark.setAttribute('role','img');mark.setAttribute('aria-label','In voice chat');target.append(mark);}
      }else badge?.remove();
    }
    window.dispatchEvent(new Event('infuser-voice-layout'));
  }

  const observer=new MutationObserver(()=>{
    const ids=new Set((snapshot?.participants||[]).map(p=>p.id));
    for(const person of document.querySelectorAll('.presence-person[data-player-id]')){
      const target=person.querySelector('summary');if(!target)continue;
      const badge=target.querySelector('.voice-badge');
      const pid=person.dataset.playerId;
      person.classList.toggle('voice-speaking',speakingIds.has(pid));
      if(ids.has(pid)){
        if(!badge){const mark=document.createElement('span');mark.className='voice-badge';mark.innerHTML=icon;mark.title='In voice chat';mark.setAttribute('role','img');mark.setAttribute('aria-label','In voice chat');target.append(mark);}
      }else badge?.remove();
    }
  });
  observer.observe(document.getElementById('friends'),{childList:true,subtree:true});

  async function refreshDevices(){
    if(!navigator.mediaDevices?.enumerateDevices)return;
    try{
      const all=await navigator.mediaDevices.enumerateDevices();
      deviceList={
        inputs:all.filter(d=>d.kind==='audioinput'),
        outputs:all.filter(d=>d.kind==='audiooutput'),
      };
      if(devices.inputId&&!deviceList.inputs.some(d=>d.deviceId===devices.inputId)){devices.inputId='';saveDevices(devices);}
      if(devices.outputId&&!deviceList.outputs.some(d=>d.deviceId===devices.outputId)){devices.outputId='';saveDevices(devices);}
      if(mixer.matches(':popover-open')&&optionsOpen)renderMixer();
    }catch{/* Permission or device enumeration unavailable. */}
  }
  if(navigator.mediaDevices?.addEventListener){
    navigator.mediaDevices.addEventListener('devicechange',()=>{void refreshDevices();});
  }

  async function stopMicTest(){
    if(!testingMic)return;
    testingMic=false;
    try{await testProcessor?.destroy();}catch{/* ignore */}
    testProcessor=null;
    if(testStream){for(const t of testStream.getTracks())t.stop();testStream=null;}
    closeVoiceContextIfIdle();
    if(mixer.matches(':popover-open')){lastMixerSig='';renderMixer();}
  }

  async function toggleMicTest(){
    if(testingMic){await stopMicTest();return;}
    if(phase!=='off'){notice('Leave voice chat first to run a microphone test.');return;}
    try{
      if(!navigator.mediaDevices?.getUserMedia)throw Error('This browser cannot use a microphone here.');
      const ctx=ensureVoiceContext();
      if(!ctx)throw Error('Audio is unavailable in this browser.');
      audioMod=audioMod||await audioModPromise;
      const stream=await navigator.mediaDevices.getUserMedia({
        audio:{
          echoCancellation:true,
          noiseSuppression:settings.noise_suppression,
          autoGainControl:settings.auto_gain_control,
          ...(devices.inputId?{deviceId:{exact:devices.inputId}}:{}),
        }
      });
      testStream=stream;
      const proc=audioMod.createVoiceInputProcessor(pctToGain(settings.input_gain));
      await proc.init({track:stream.getAudioTracks()[0],audioContext:ctx});
      proc.setLoopback(true);
      testProcessor=proc;
      testingMic=true;
      void refreshDevices();
      lastMixerSig='';renderMixer();startMeterLoop();
    }catch(error){
      await stopMicTest();
      notice(error.name==='NotAllowedError'?'Allow microphone access to test your microphone.':error.message||'Could not start microphone test.');
    }
  }

  function stopMeterLoop(){
    if(meterFrame){cancelAnimationFrame(meterFrame);meterFrame=0;}
  }

  function startMeterLoop(){
    stopMeterLoop();
    if(!mixer.matches(':popover-open')||document.hidden)return;
    const step=ts=>{
      if(!mixer.matches(':popover-open')||document.hidden){meterFrame=0;return;}
      if(ts-lastMeterAt>=50){
        lastMeterAt=ts;
        const activeProc=testingMic?testProcessor:(phase==='on'&&!micMuted&&!deafened?inputProcessor:null);
        const reading=activeProc?activeProc.getMeter():{level:0,clipping:false};
        if(meterFill){
          meterFill.style.transform=`scaleX(${reading.level.toFixed(3)})`;
          meterFill.dataset.clip=String(Boolean(reading.clipping));
        }
      }
      meterFrame=requestAnimationFrame(step);
    };
    meterFrame=requestAnimationFrame(step);
  }

  function positionMixer(){
    if(!mixer.matches(':popover-open'))return;
    const fallback=split?.caretBtn||split?.root;
    const anchor=(mixerAnchor&&mixerAnchor.isConnected&&!mixerAnchor.hidden)?mixerAnchor:fallback;
    if(!anchor)return;
    const bounds=anchor.getBoundingClientRect();
    const width=Math.min(320,innerWidth-24);
    mixer.style.width=width+'px';
    mixer.style.left=Math.max(12,Math.min(bounds.right-width,innerWidth-width-12))+'px';
    mixer.style.top='0px';
    mixer.style.maxHeight=Math.max(160,innerHeight-24)+'px';
    const height=mixer.getBoundingClientRect().height;
    const top=bounds.bottom+8+height<=innerHeight-12?bounds.bottom+8:Math.max(12,bounds.top-height-8);
    mixer.style.top=top+'px';
  }

  function buildSlider({label,value,onInput,onCommit,extra}){
    const wrap=document.createElement('div');wrap.className='vm-slider-row';
    const top=document.createElement('div');top.className='vm-slider-top';
    const name=document.createElement('span');name.textContent=label;
    const pct=document.createElement('button');pct.type='button';pct.className='vm-pct';pct.textContent=`${value}%`;pct.title='Reset to 100%';
    top.append(name,pct);
    const input=document.createElement('input');
    input.type='range';input.min='0';input.max='200';input.step='5';input.value=String(value);input.className='vm-range';
    input.setAttribute('aria-label',label);
    input.addEventListener('pointerdown',()=>{dragging=true;});
    input.addEventListener('pointerup',()=>{dragging=false;});
    input.addEventListener('pointercancel',()=>{dragging=false;});
    input.addEventListener('input',()=>{const next=clampPct(input.value);pct.textContent=`${next}%`;onInput(next);});
    input.addEventListener('change',()=>{dragging=false;const next=clampPct(input.value);pct.textContent=`${next}%`;onCommit(next);});
    const reset=()=>{input.value='100';pct.textContent='100%';onCommit(100);};
    input.addEventListener('dblclick',reset);pct.addEventListener('click',reset);
    wrap.append(top,input);
    if(extra)wrap.append(extra);
    return wrap;
  }

  function devicesSig(){
    return JSON.stringify([
      deviceList.inputs.map(d=>[d.deviceId,d.label]),
      deviceList.outputs.map(d=>[d.deviceId,d.label]),
    ]);
  }

  function mixerSig(peers=(snapshot?.participants||[]).filter(p=>p.id!==me.player_id)){
    return JSON.stringify([
      phase,Boolean(snapshot?.self),micMuted,deafened,testingMic,optionsOpen,
      settings,peers.map(p=>[p.id,p.name,speakingIds.has(p.id)]),
      devices,devicesSig(),
    ]);
  }

  function renderMixer(){
    if(!mixer.matches(':popover-open'))return;
    const on=phase!=='off'||Boolean(snapshot?.self);
    const peers=(snapshot?.participants||[]).filter(p=>p.id!==me.player_id);
    const curDevicesSig=devicesSig();
    const sig=mixerSig(peers);
    if(sig===lastMixerSig)return;
    lastMixerSig=sig;
    renderedDevicesSig=curDevicesSig;

    mixer.replaceChildren();
    const head=document.createElement('div');head.className='vm-head';
    const statusText=phase==='on'?'Voice connected':phase==='connecting'?'Connecting…':phase==='reconnecting'?'Reconnecting…':snapshot?.self?'Voice on in another tab':'Voice settings';
    const title=document.createElement('strong');title.textContent=statusText;
    const close=document.createElement('button');close.type='button';close.className='close vm-close';close.textContent='×';close.setAttribute('aria-label','Close voice settings');
    close.onclick=()=>mixer.hidePopover();
    head.append(title,close);
    mixer.append(head);

    const actions=document.createElement('div');actions.className='vm-actions';
    if(on){
      const effectiveMuted=Boolean(micMuted||deafened);
      const qMute=document.createElement('button');qMute.type='button';qMute.className='vm-action';
      qMute.dataset.active=String(effectiveMuted);qMute.setAttribute('aria-pressed',String(effectiveMuted));
      qMute.innerHTML=(effectiveMuted?micOffIcon:micOnIcon)+`<span>${effectiveMuted?'Unmute':'Mute'}</span>`;
      qMute.onclick=()=>{
        if(deafened)queuePatch({mic_muted:false,deafened:false},{immediate:true});
        else queuePatch({mic_muted:!micMuted},{immediate:true});
        lastMixerSig='';renderMixer();
      };

      const qDeafen=document.createElement('button');qDeafen.type='button';qDeafen.className='vm-action';
      qDeafen.dataset.active=String(deafened);qDeafen.setAttribute('aria-pressed',String(deafened));
      qDeafen.innerHTML=(deafened?deafenIcon:icon)+`<span>${deafened?'Undeafen':'Deafen'}</span>`;
      qDeafen.onclick=()=>{
        queuePatch({deafened:!deafened},{immediate:true});
        lastMixerSig='';renderMixer();
      };

      const qLeave=document.createElement('button');qLeave.type='button';qLeave.className='vm-action vm-leave';
      qLeave.textContent='Leave';
      qLeave.onclick=()=>{void stop({reason:'user_toggle'});mixer.hidePopover();};
      actions.append(qMute,qDeafen,qLeave);
    }else{
      const qJoin=document.createElement('button');qJoin.type='button';qJoin.className='vm-join';
      qJoin.textContent='Join voice chat';
      qJoin.disabled=snapshot?.available===false;
      qJoin.onclick=()=>{void start();};
      actions.append(qJoin);
    }
    mixer.append(actions);

    const meter=document.createElement('div');meter.className='vm-meter';meter.setAttribute('aria-hidden','true');
    meterFill=document.createElement('div');meterFill.className='vm-meter-fill';
    meter.append(meterFill);

    mixer.append(
      buildSlider({
        label:'Microphone',
        value:settings.input_gain,
        onInput:v=>queuePatch({input_gain:v}),
        onCommit:v=>queuePatch({input_gain:v},{immediate:true}),
        extra:meter,
      }),
      buildSlider({
        label:'Voice volume',
        value:settings.output_volume,
        onInput:v=>queuePatch({output_volume:v}),
        onCommit:v=>queuePatch({output_volume:v},{immediate:true}),
      })
    );

    if(peers.length){
      const sec=document.createElement('div');sec.className='vm-peers';
      const heading=document.createElement('p');heading.className='vm-section-title';heading.textContent='People in voice';
      sec.append(heading);
      for(const peer of peers){
        const pref=peerPref(peer.id);
        const row=document.createElement('div');row.className='vm-peer';
        if(speakingIds.has(peer.id))row.classList.add('voice-speaking');
        const top=document.createElement('div');top.className='vm-peer-top';
        const dot=document.createElement('span');dot.className='vm-speak-dot';dot.setAttribute('aria-hidden','true');
        const name=document.createElement('span');name.className='vm-peer-name';name.textContent=peer.name;
        const pct=document.createElement('button');pct.type='button';pct.className='vm-pct';pct.textContent=`${pref.volume}%`;pct.title='Reset to 100%';
        const pMute=document.createElement('button');pMute.type='button';pMute.className='vm-peer-mute';
        pMute.dataset.muted=String(pref.muted);pMute.setAttribute('aria-pressed',String(pref.muted));
        pMute.setAttribute('aria-label',`${pref.muted?'Unmute':'Mute'} ${peer.name}`);
        pMute.title=pref.muted?`Unmute ${peer.name}`:`Mute ${peer.name}`;
        pMute.innerHTML=pref.muted?speakerOffIcon:speakerOnIcon;
        pMute.onclick=()=>{
          queuePatch({peer_volumes:{[peer.id]:{muted:!pref.muted}}},{immediate:true});
          lastMixerSig='';renderMixer();
        };
        top.append(dot,name,pct,pMute);

        const slider=document.createElement('input');
        slider.type='range';slider.min='0';slider.max='200';slider.step='5';slider.value=String(pref.volume);
        slider.className='vm-range';slider.setAttribute('aria-label',`${peer.name} volume`);
        slider.addEventListener('pointerdown',()=>{dragging=true;});
        slider.addEventListener('pointerup',()=>{dragging=false;});
        slider.addEventListener('pointercancel',()=>{dragging=false;});
        slider.addEventListener('input',()=>{const next=clampPct(slider.value);pct.textContent=`${next}%`;queuePatch({peer_volumes:{[peer.id]:{volume:next}}});});
        slider.addEventListener('change',()=>{dragging=false;const next=clampPct(slider.value);pct.textContent=`${next}%`;queuePatch({peer_volumes:{[peer.id]:{volume:next}}},{immediate:true});});
        const resetPeer=()=>{slider.value='100';pct.textContent='100%';queuePatch({peer_volumes:{[peer.id]:{volume:100,muted:false}}},{immediate:true});lastMixerSig='';renderMixer();};
        slider.addEventListener('dblclick',resetPeer);pct.addEventListener('click',resetPeer);
        row.append(top,slider);sec.append(row);
      }
      mixer.append(sec);
    }

    const advanced=document.createElement('details');advanced.className='vm-advanced';advanced.open=optionsOpen;
    const sum=document.createElement('summary');sum.textContent='Audio options';
    advanced.addEventListener('toggle',()=>{
      if(advanced.open===optionsOpen)return;
      optionsOpen=advanced.open;
      if(optionsOpen){
        if(devicesSig()!==renderedDevicesSig){
          renderMixer();
        }else{
          lastMixerSig=mixerSig();
        }
        void refreshDevices();
      }else{
        lastMixerSig=mixerSig();
      }
      positionMixer();
    });
    const body=document.createElement('div');body.className='vm-advanced-body';

    if(deviceList.inputs.length){
      const lbl=document.createElement('label');lbl.className='vm-field';lbl.textContent='Microphone device';
      const sel=document.createElement('select');
      const def=document.createElement('option');def.value='';def.textContent='System default';sel.append(def);
      for(const d of deviceList.inputs){
        const opt=document.createElement('option');opt.value=d.deviceId;opt.textContent=d.label||'Microphone';sel.append(opt);
      }
      sel.value=devices.inputId;
      sel.onchange=()=>{devices.inputId=sel.value;saveDevices(devices);void applyTrackConstraints();};
      lbl.append(sel);body.append(lbl);
    }

    const canSink=typeof HTMLMediaElement!=='undefined'&&'setSinkId' in HTMLMediaElement.prototype;
    if(canSink&&deviceList.outputs.length){
      const lbl=document.createElement('label');lbl.className='vm-field';lbl.textContent='Speaker device';
      const sel=document.createElement('select');
      const def=document.createElement('option');def.value='';def.textContent='System default';sel.append(def);
      for(const d of deviceList.outputs){
        const opt=document.createElement('option');opt.value=d.deviceId;opt.textContent=d.label||'Speakers';sel.append(opt);
      }
      sel.value=devices.outputId;
      sel.onchange=()=>{devices.outputId=sel.value;saveDevices(devices);void applyOutputDevice();};
      lbl.append(sel);body.append(lbl);
    }

    for(const [key,text] of [
      ['auto_gain_control','Auto gain control'],
      ['noise_suppression','Noise suppression'],
    ]){
      const lbl=document.createElement('label');lbl.className='vm-check';
      const box=document.createElement('input');box.type='checkbox';box.checked=Boolean(settings[key]);
      box.onchange=()=>{queuePatch({[key]:box.checked},{immediate:true,reapplyConstraints:true});};
      lbl.append(box,document.createTextNode(text));
      body.append(lbl);
    }

    if(phase==='off'){
      const testBtn=document.createElement('button');testBtn.type='button';testBtn.className='secondary vm-test';
      testBtn.textContent=testingMic?'Stop mic test':'Test microphone';
      testBtn.onclick=()=>{void toggleMicTest();};
      body.append(testBtn);
    }
    advanced.append(sum,body);
    mixer.append(advanced);

    const footer=document.createElement('div');footer.className='vm-footer';
    const resetAll=document.createElement('button');resetAll.type='button';resetAll.className='text-button';
    resetAll.textContent='Reset levels';
    resetAll.onclick=()=>{
      const peerReset={};
      for(const pid of Object.keys(settings.peer_volumes||{}))peerReset[pid]={volume:100,muted:false};
      queuePatch({input_gain:100,output_volume:100,peer_volumes:peerReset},{immediate:true});
      lastMixerSig='';renderMixer();
    };
    footer.append(resetAll);
    mixer.append(footer);
    positionMixer();
  }

  function openSettings(anchor=split?.caretBtn){
    if(!snapshot?.configured)return;
    mixerAnchor=anchor||split?.caretBtn||null;
    if(mixer.matches(':popover-open')){mixer.hidePopover();return;}
    mixer.showPopover();
    lastMixerSig='';
    renderMixer();
    mixer.querySelector('.vm-action:not(:disabled), .vm-join:not(:disabled), .vm-close')?.focus({preventScroll:true});
    startMeterLoop();
    void refreshDevices();
  }

  mixer.addEventListener('beforetoggle',event=>{
    const isOpen=event.newState==='open';
    paint(isOpen);
    if(!isOpen&&(mixer.contains(document.activeElement)||document.activeElement===document.body)){
      mixerAnchor?.focus?.({preventScroll:true});
    }
  });
  mixer.addEventListener('toggle',()=>{
    const isOpen=mixer.matches(':popover-open');
    paint();
    if(isOpen){positionMixer();startMeterLoop();}
    else{stopMeterLoop();void stopMicTest();}
  });
  mixer.addEventListener('keydown',event=>{
    if(event.key==='Escape'){event.preventDefault();event.stopPropagation();mixer.hidePopover();mixerAnchor?.focus?.({preventScroll:true});}
  });
  window.addEventListener('resize',positionMixer);
  window.addEventListener('scroll',positionMixer,{passive:true});

  function stopControlLoop(){
    controlAbort?.abort();controlAbort=null;controlRunning=false;
  }

  function ensureControlLoop(){
    if(alive&&phase!=='off'&&!controlRunning)void runControlLoop(generation);
  }

  async function runControlLoop(myGen){
    if(controlRunning)return;
    controlRunning=true;
    while(alive&&myGen===generation&&phase!=='off'){
      const ac=new AbortController();controlAbort=ac;
      try{
        const query=`since=${encodeURIComponent(String(revision))}&epoch=${encodeURIComponent(serverEpoch)}`;
        const res=await fetch('/api/voice/control?'+query,{
          credentials:'same-origin',
          headers:{...(()=>{try{const p=sessionStorage.getItem('infuser.profile')||me.player_id;return p?{'X-Infuser-Profile':p}:{};}catch{return me.player_id?{'X-Infuser-Profile':me.player_id}:{};}})(),...(()=>{try{const v=sessionStorage.getItem('infuser.visit');return v?{'X-Infuser-Visit':v}:{};}catch{return {};}})()},
          signal:AbortSignal.any([ac.signal,AbortSignal.timeout(30000)]),
        });
        if(!alive||myGen!==generation)break;
        diagnostics?.link('voice-control').set(res.ok?'up':'down',res.ok?undefined:'network');
        if(res.status===204)continue;
        if(res.status===401||res.status===403){
          await stop({reason:'access_lost',message:'Voice chat ended. Your access changed.'});
          break;
        }
        if(res.ok){
          const data=await res.json();
          syncServerState(data);
          if(lease&&phase!=='connecting'&&data.self?.id!==lease){
            await stop({reason:'server_ended',remote:false,message:'Voice chat was turned off.'});
            break;
          }
          paint();
          if(mixer.matches(':popover-open')&&!dragging)renderMixer();
        }else{
          // End without a timed backoff; status poll, online, or visibilitychange restarts.
          break;
        }
      }catch{
        if(!ac.signal.aborted)diagnostics?.link('voice-control').set('down','network');
        break;
      }
    }
    controlRunning=false;
  }


  async function stop({reason,remote=true,message}){
    if(reason!=='pagehide'){
      const cause=reason==='user_toggle'?'user':reason;
      if(phase!=='off')diagnostics?.link('voice').set('ended',cause,{session:diagnosticSession});
      diagnostics?.link('voice-control').set('ended',cause);
      joinOp?.end('abandoned',cause);
    }
    generation++;stopControlLoop();
    const old=lease||snapshot?.self?.id;lease=null;
    const current=room,wasConnected=phase==='on'||phase==='reconnecting';room=null;phase='off';
    revision=0;serverEpoch='';lastAckedKey='';
    micMuted=false;deafened=false;speakingIds.clear();
    try{await inputProcessor?.destroy();}catch{/* ignore */}
    inputProcessor=null;
    outputController?.clear();outputController=null;
    track?.stop();track=null;
    if(alive&&wasConnected)chimes.play(false);
    chimes.close(!alive);
    closeVoiceContextIfIdle();
    if(snapshot){snapshot.self=null;snapshot.participants=snapshot.participants.filter(p=>p.id!==me.player_id);}
    audio.replaceChildren();paint();
    if(mixer.matches(':popover-open')){lastMixerSig='';renderMixer();}
    if(current)void current.disconnect();
    if(remote&&old){try{await request('/api/voice',{method:'DELETE',body:JSON.stringify({id:old}),keepalive:true});}catch{/* The server's expiring lease and reconciler also remove abandoned connections. */}}
    if(message)notice(message);
  }

  async function attachInputProcessor(mic,mod,fallbackCtx){
    const procCtx=mic.audioContext||fallbackCtx;
    if(!procCtx||typeof mic.setProcessor!=='function')return;
    try{
      if(!mic.audioContext&&typeof mic.setAudioContext==='function')await mic.setAudioContext(procCtx);
      const proc=mod.createVoiceInputProcessor(pctToGain(settings.input_gain));
      await mic.setProcessor(proc);
      inputProcessor=proc;
    }catch{/* Fallback to unprocessed mic track if Web Audio processor fails. */}
  }

  let diagnosticSession,joinOp;
  async function start(){
    diagnosticSession=undefined;
    joinOp=diagnostics?.op('voice.join');joinOp?.stage('microphone');diagnostics?.link('voice').set('connecting');
    await stopMicTest();
    // Start audio in the button's user gesture, before microphone/network awaits.
    chimes.unlock();
    const ctx=ensureVoiceContext();
    const attempt=++generation;phase='connecting';micMuted=false;deafened=false;paint();
    try {
      if(!navigator.mediaDevices?.getUserMedia)throw Error('This browser cannot use a microphone here.');
      const [{Room,RoomEvent,DisconnectReason,createLocalAudioTrack},mod]=await Promise.all([
        import('/assets/vendor/livekit-2.24.1.js'),
        audioModPromise,
      ]);
      audioMod=mod;
      if(attempt!==generation)return;
      const mic=await createLocalAudioTrack({
        echoCancellation:true,
        noiseSuppression:settings.noise_suppression,
        autoGainControl:settings.auto_gain_control,
        ...(devices.inputId?{deviceId:devices.inputId}:{}),
      });
      if(attempt!==generation){mic.stop();return;}track=mic;
      const id=crypto.randomUUID();lease=id;
      joinOp?.stage('grant');
      const grant=await request('/api/voice',{method:'POST',body:JSON.stringify({id})});
      if(attempt!==generation){await request('/api/voice',{method:'DELETE',body:JSON.stringify({id})});return;}
      lease=grant.id;diagnosticSession=grant.diagnostic_session;
      if(grant.epoch){serverEpoch=grant.epoch;revision=Number(grant.revision||0);lastAckedKey='';}
      outputController=mod.createOutputController({getAudioContext:ensureVoiceContext});
      if(devices.outputId)void outputController.setSinkId(devices.outputId);

      const current=new Room({singlePeerConnection:false,disconnectOnPageLeave:true});room=current;
      let participants=new Set();
      const roster=()=>{participants=new Set(current.remoteParticipants.keys());};
      current.on(RoomEvent.ParticipantConnected,p=>{
        if(room!==current)return;
        const added=!participants.has(p.identity);participants.add(p.identity);
        if(added&&phase==='on'){chimes.play(true);void poll();}
      });
      current.on(RoomEvent.ParticipantDisconnected,p=>{
        if(room!==current)return;
        outputController?.unregister(p.identity);
        if(participants.delete(p.identity)&&phase==='on'){chimes.play(false);void poll();}
      });
      current.on(RoomEvent.TrackSubscribed,(t,_pub,participant)=>{
        if(t.kind==='audio'){
          const element=t.attach();element.autoplay=true;audio.append(element);
          const identity=participant?.identity;
          if(identity&&outputController){
            outputController.register(identity,t,gainForIdentity(identity));
            if(!identityToPlayer.has(identity))void poll();
          }
        }
      });
      current.on(RoomEvent.TrackUnsubscribed,(t,_pub,participant)=>{
        if(participant?.identity)outputController?.unregister(participant.identity);
        t.detach().forEach(el=>el.remove());
      });
      current.on(RoomEvent.ActiveSpeakersChanged,speakers=>{
        if(room!==current)return;
        const next=new Set();
        for(const s of speakers||[]){
          const pid=identityToPlayer.get(s.identity);
          if(pid)next.add(pid);
        }
        speakingIds=next;
        paint();
        if(mixer.matches(':popover-open')&&!dragging)renderMixer();
      });
      current.on(RoomEvent.Reconnecting,()=>{if(room===current){phase='reconnecting';diagnostics?.link('voice').set('degraded','network',{session:diagnosticSession});chimes.quiet();paint();}});
      current.on(RoomEvent.Reconnected,()=>{
        if(room!==current)return;
        roster();chimes.quiet();phase='on';diagnostics?.link('voice').set('up',undefined,{session:diagnosticSession});
        if(track&&!track.processor)void attachInputProcessor(track,mod,ensureVoiceContext()).then(()=>applyAudioState({ack:true}));
        else void applyAudioState({ack:true});
        paint();
      });
      const session=diagnosticSession;
      current.once(RoomEvent.Disconnected,reason=>{
        if(room!==current)return;
        const cause=DisconnectReason[reason]==='ROOM_DELETED'?'server_ended':'network';
        diagnostics?.link('voice').set('down',cause,{session});
        void stop({reason:cause,message:'Voice chat disconnected. Tap the headset to join again.'});
      });
      joinOp?.stage('connect');
      await current.connect(grant.url,grant.token,{autoSubscribe:true,maxRetries:1});
      if(attempt!==generation){void current.disconnect();return;}
      joinOp?.stage('publish');
      await current.localParticipant.publishTrack(mic);
      joinOp?.stage('playback');
      await current.startAudio();
      if(attempt!==generation)return;
      await attachInputProcessor(mic,mod,ctx);
      if(attempt!==generation)return;
      roster();phase='on';joinOp?.end('ok',undefined,{session:diagnosticSession});diagnostics?.link('voice').set('up',undefined,{session:diagnosticSession});heartbeatAt=0;
      syncIdentityMap();
      await applyAudioState({ack:true});
      chimes.play(true);paint();
      void poll();
      void runControlLoop(attempt);
      if(mixer.matches(':popover-open')){lastMixerSig='';renderMixer();startMeterLoop();}
    }catch(error){
      if(attempt!==generation)return;
      joinOp?.end('fail','error:'+error.name,{session:diagnosticSession});
      const message=error.name==='NotAllowedError'?'Allow microphone access in your browser to use voice chat.':error.name==='NotFoundError'?'Connect a microphone to use voice chat.':error.message||'Could not connect to voice chat.';
      await stop({reason:'error',message});
    }
  }

  function renderFriendControls(person,panel){
    if(!snapshot?.configured||!person?.id||person.id===me.player_id)return;
    const inVoice=(snapshot.participants||[]).some(p=>p.id===person.id);
    const hasCustom=Boolean(settings.peer_volumes?.[person.id]);
    if(!inVoice&&!hasCustom)return;
    const pref=peerPref(person.id);
    const wrap=document.createElement('div');wrap.className='friend-voice';
    const top=document.createElement('div');top.className='vm-peer-top';
    const title=document.createElement('span');title.className='quiet';title.textContent='Voice volume';
    const pct=document.createElement('button');pct.type='button';pct.className='vm-pct';pct.textContent=`${pref.volume}%`;pct.title='Reset to 100%';
    const fMute=document.createElement('button');fMute.type='button';fMute.className='vm-peer-mute';
    fMute.dataset.muted=String(pref.muted);fMute.setAttribute('aria-pressed',String(pref.muted));
    fMute.setAttribute('aria-label',`${pref.muted?'Unmute':'Mute'} ${person.name}`);
    fMute.innerHTML=pref.muted?speakerOffIcon:speakerOnIcon;
    fMute.onclick=()=>{
      const nextMuted=!peerPref(person.id).muted;
      queuePatch({peer_volumes:{[person.id]:{muted:nextMuted}}},{immediate:true});
      fMute.dataset.muted=String(nextMuted);fMute.setAttribute('aria-pressed',String(nextMuted));
      fMute.innerHTML=nextMuted?speakerOffIcon:speakerOnIcon;
    };
    top.append(title,pct,fMute);
    const slider=document.createElement('input');
    slider.type='range';slider.min='0';slider.max='200';slider.step='5';slider.value=String(pref.volume);
    slider.className='vm-range';slider.setAttribute('aria-label',`${person.name} voice volume`);
    slider.addEventListener('pointerdown',()=>{dragging=true;});
    slider.addEventListener('pointerup',()=>{dragging=false;});
    slider.addEventListener('pointercancel',()=>{dragging=false;});
    slider.addEventListener('input',()=>{const next=clampPct(slider.value);pct.textContent=`${next}%`;queuePatch({peer_volumes:{[person.id]:{volume:next}}});});
    slider.addEventListener('change',()=>{dragging=false;const next=clampPct(slider.value);pct.textContent=`${next}%`;queuePatch({peer_volumes:{[person.id]:{volume:next}}},{immediate:true});});
    const reset=()=>{slider.value='100';pct.textContent='100%';queuePatch({peer_volumes:{[person.id]:{volume:100,muted:false}}},{immediate:true});};
    slider.addEventListener('dblclick',reset);pct.addEventListener('click',reset);
    wrap.append(top,slider);
    panel.append(wrap);
  }

  async function poll(){
    if(polling||!alive)return;polling=true;
    try{
      if(!audioMod)audioMod=await audioModPromise.catch(()=>null);
      if(!split)await ensureSplit().catch(()=>{});
      if(lease&&room&&Date.now()-heartbeatAt>20000){
        await request('/api/voice/heartbeat',{method:'POST',body:JSON.stringify({id:lease})});heartbeatAt=Date.now();
      }
      const status=await request('/api/voice');snapshot=status;
      syncIdentityMap();
      syncServerState(status);
      if(!status.configured)clearInterval(timer);
      if(room&&phase!=='connecting')phase=room.state==='connected'&&status.available?'on':'reconnecting';
      if(lease&&phase!=='connecting'&&status.self?.id!==lease){await stop({reason:'server_ended',remote:false,message:'Voice chat was turned off.'});}
      ensureControlLoop();
      paint();
      if(mixer.matches(':popover-open')&&!dragging)renderMixer();
    }catch(error){
      if(lease&&[401,403].includes(error.status))await stop({reason:'access_lost',message:'Voice chat ended. Your access changed.'});
      else if(room){phase='reconnecting';paint();}
    }finally{polling=false;}
  }
  // Page navigation (including profile/visit changes) stops capture immediately.
  // A pinned request retains the old profile headers while navigation proceeds.
  addEventListener('pagehide',()=>{alive=false;observer.disconnect();clearInterval(timer);clearTimeout(patchTimer);stopMeterLoop();void stopMicTest();void stop({reason:'pagehide',remote:!!lease});});
  const timer=setInterval(()=>{if(lease||!document.hidden)void poll();},3000);
  window.addEventListener('online',()=>{if(alive){ensureControlLoop();void poll();}});
  document.addEventListener('visibilitychange',()=>{if(!document.hidden){ensureControlLoop();void poll();if(mixer.matches(':popover-open'))startMeterLoop();}else stopMeterLoop();});
  void poll();
  return {stop:()=>stop({reason:'user_toggle'}),openSettings,renderFriendControls};
}
