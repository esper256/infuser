import { initChatEmoji } from '/assets/chat-emoji.js';
import { seenLimit, advancedRead, placeReads } from '/assets/chat-reads.js';
import { attachmentIcon, attachmentDraft, screenshotTile } from '/assets/screenshots.js';
import { openHomeChat } from '/assets/groups.js';
// A theme opts in from CSS. Unset means the horizontal row and the upward tails.
export function chatDock(){
  const style=getComputedStyle(document.documentElement);
  let stack=style.getPropertyValue('--chat-stack').trim();
  let anchor=style.getPropertyValue('--chat-anchor').trim();
  if(stack!=='row'&&stack!=='column')stack='';
  if(!['below','above','left','right'].includes(anchor))anchor='';
  if(!stack&&!anchor)return null;
  if(!stack)stack=anchor==='left'||anchor==='right'?'column':'row';
  if(!anchor)anchor=stack==='column'?'right':'below';
  if(stack==='row'&&anchor==='below')return null;
  const gap=parseFloat(style.getPropertyValue('--chat-gap'));
  return {stack,anchor,gap:Number.isFinite(gap)?gap:18};
}
const graphemes=typeof Intl.Segmenter==='function'?new Intl.Segmenter(undefined,{granularity:'grapheme'}):null;
const emojiPresentation=/\p{Emoji_Presentation}|\p{Emoji}\uFE0F|[#*0-9]\uFE0F?\u20E3/u;
function messageText(value){
  const text=document.createElement('p');
  if(!graphemes||!emojiPresentation.test(value)){text.textContent=value;return text;}
  let plain='';
  for(const {segment} of graphemes.segment(value)){
    // Respect explicit text presentation and keep joined emoji sequences intact.
    if(!segment.includes('\uFE0E')&&emojiPresentation.test(segment)){
      if(plain){text.append(plain);plain='';}
      const emoji=document.createElement('span');emoji.className='chat-inline-emoji';emoji.textContent=segment;text.append(emoji);
    }else plain+=segment;
  }
  if(plain)text.append(plain);
  return text;
}
// Shared Library room. All message text stays text, never markup or links.
export function initChat({me, api, profileAvatar,onSpeakerContext=()=>{}}) {
  const request=(path,options={})=>api(path,{...options,signal:AbortSignal.timeout(12000)});
  const emptyMessage=me.inspecting?'No messages yet.':'No messages yet. Start the conversation.';
  const root=document.createElement('aside');root.id='library-chat';root.setAttribute('aria-label','Library chat');
  root.innerHTML=`<button id="chat-toggle" type="button" aria-expanded="false" aria-controls="chat-panel" title="Show chat"><span class="chat-icon" aria-hidden="true">···</span><span class="sr-only">Chat</span><span id="chat-unread" hidden></span></button>
    <div id="chat-pointers" aria-hidden="true"></div>
    <section id="chat-panel" aria-label="Chat" hidden>
      <button id="chat-close" type="button" aria-label="Close chat" title="Close chat">×</button>
      <div id="chat-log" role="log" aria-label="Messages" aria-live="off" tabindex="0"><p class="chat-empty">${emptyMessage}</p></div>
      <button id="chat-new" class="chat-new" type="button" hidden>New messages ↓</button>
      <p id="chat-connection" role="status"></p><p id="chat-error" role="alert"></p>
      <div class="chat-compose-row">
        <form id="chat-form"><label class="sr-only" for="chat-text">Message</label><div id="chat-input-scroll"><textarea id="chat-text" maxlength="280" rows="2" placeholder="Message…" aria-describedby="chat-help chat-limit"></textarea><div id="chat-scrollbar" aria-hidden="true" tabindex="-1" hidden><div id="chat-scroll-range"></div></div></div><span id="chat-help" class="sr-only">280 characters maximum. Enter to send; Shift+Enter for a new line.</span><button id="chat-emoji-toggle" type="button" aria-label="Choose emoji" title="Choose emoji" aria-expanded="false" aria-controls="chat-emoji-panel" aria-haspopup="dialog"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M8 14c1 4 7 4 8 0M8 8h.01M16 8h.01"/></svg></button><button id="chat-send" type="submit" aria-label="Send message" title="Send message" disabled><svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M2 3v7l15 2-15 2v7l21-9z"/></svg></button><span id="chat-limit" hidden></span></form>
      </div>
      <section id="chat-emoji-panel" role="dialog" aria-label="Choose emoji" hidden><div class="chat-emoji-heading"><strong>Emoji</strong><button class="chat-emoji-close" type="button" aria-label="Close emoji picker">×</button></div><p role="status"></p><div class="chat-emoji-mount"></div></section>
      <span id="chat-announcement" class="sr-only" role="status"></span>
    </section>
    <div id="chat-actions" popover="manual" role="menu" aria-label="Message actions"><button id="chat-delete" type="button" role="menuitem">Delete message</button><button id="chat-cancel-action" type="button" role="menuitem">Cancel</button></div>`;
  document.body.append(root);
  const $=id=>document.getElementById('chat-'+id), log=$('log'), input=$('text');
  let screenshot=null;
  $('emoji-toggle').innerHTML=attachmentIcon;$('emoji-toggle').setAttribute('aria-label','Choose emoji or screenshot');$('emoji-toggle').title='Emoji or screenshot';$('emoji-panel').setAttribute('aria-label','Choose emoji or screenshot');
  const draft=document.createElement('div');draft.className='screenshot-draft';draft.hidden=true;$('form').before(draft);
  function attach(id){if(sending)return;screenshot=id;attachmentDraft(draft,id,request,()=>attach(null));input.oninput();}
  const emojiPicker=initChatEmoji({root,input,button:$('emoji-toggle'),panel:$('emoji-panel'),api:request,onScreenshot:attach});
  if(me.inspecting)$('form').hidden=true;
  const presence=document.getElementById('friends'), cluster=document.createElement('div');cluster.className='presence-chat';
  presence.before(cluster);cluster.append(presence,$('toggle'));root.hidden=true;
  let messages=[], reads=[], initialized=false, busy=false, sending=false, timer, retry, lastRead=0,readAck=0,readFlight=0,readHold=false,positionFrame,positionSignature,speakerSignature,scrollDirection=0,scrollSpeed=0,lastScrollTop=0,lastScrollAt=0;
  const motion=matchMedia('(prefers-reduced-motion: reduce)');
  const setText=(node,text)=>{if(node.textContent!==text)node.textContent=text;};
  const setHidden=(node,hidden)=>{if(node.hidden!==hidden)node.hidden=hidden;};
  function schedulePosition(){
    if(positionFrame||!isOpen())return;
    positionFrame=requestAnimationFrame(()=>{positionFrame=null;position();});
  }
  // Only allow rounding error, not a "near bottom" zone that traps small gestures.
  const bottom=()=>log.scrollHeight-log.scrollTop-log.clientHeight<=1;
  const isOpen=()=>!$('panel').hidden;
  const compactQuery=matchMedia('(max-width: 650px), (pointer: coarse) and (max-height: 500px)');
  const compact=()=>compactQuery.matches;
  const hoverControls=matchMedia('(hover: hover) and (pointer: fine) and (min-width: 651px)');
  let hoverAction,press,actionTarget,deleteBusy=false,suppressReleaseClick=false;
  const actions=$('actions');
  function clearHover(){
    if(!hoverAction)return;
    clearTimeout(hoverAction.timer);hoverAction.remove.disabled=true;
    hoverAction.bubble.classList.remove('show-delete');hoverAction=null;
  }
  function revealDelete(bubble,remove){
    if(hoverAction?.bubble===bubble||!hoverControls.matches||deleteBusy||actionTarget)return;
    clearHover();bubble.classList.add('show-delete');remove.disabled=true;
    const state={bubble,remove,readyAt:performance.now()+500};hoverAction=state;
    state.timer=setTimeout(()=>{if(hoverAction===state&&bubble.isConnected&&isOpen())remove.disabled=false;},500);
  }
  function cancelPress(){if(press)clearTimeout(press.timer);press=null;}
  function closeActions(restore=false){
    const target=actionTarget;actionTarget=null;
    if(actions.matches(':popover-open'))actions.hidePopover();
    if(restore&&isOpen())(target?.bubble.isConnected?target.bubble:log).focus({preventScroll:true});
  }
  function showActions(message,bubble){
    if(!isOpen()||deleteBusy||!bubble.isConnected)return;
    clearHover();actionTarget={message,bubble};$('delete').disabled=false;
    actions.showPopover();
    const rect=bubble.getBoundingClientRect(),viewport=window.visualViewport;
    const left=viewport?.offsetLeft||0,top=viewport?.offsetTop||0,width=viewport?.width||innerWidth,height=viewport?.height||innerHeight;
    const menu=actions.getBoundingClientRect();
    actions.style.left=Math.max(left+8,Math.min(rect.right-menu.width,left+width-menu.width-8))+'px';
    actions.style.top=Math.max(top+8,Math.min(rect.bottom+6,top+height-menu.height-8))+'px';
    $('delete').focus({preventScroll:true});
  }
  async function deleteMessage(message){
    // Recheck the latest snapshot, including permissions, before acting.
    if(deleteBusy||!messages.some(m=>m.id===message.id&&m.can_delete))return;
    deleteBusy=true;clearHover();closeActions();$('delete').disabled=true;
    try{await request('/api/chat/'+message.id,{method:'DELETE'});$('error').textContent='';await poll();if(isOpen())log.focus({preventScroll:true});}
    catch(error){$('error').textContent=error.message;}
    finally{deleteBusy=false;}
  }
  $('delete').onclick=()=>{if(actionTarget)deleteMessage(actionTarget.message);};
  $('cancel-action').onclick=()=>closeActions(true);
  actions.addEventListener('keydown',event=>{
    if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeActions(true);}
    else if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){
      event.preventDefault();const buttons=[...actions.querySelectorAll('button')];
      const index=buttons.indexOf(document.activeElement);
      buttons[event.key==='Home'?0:event.key==='End'?buttons.length-1:(index+1)%buttons.length].focus();
    }else if(event.key==='Tab')closeActions();
  });
  actions.addEventListener('toggle',()=>{if(!actions.matches(':popover-open'))actionTarget=null;});
  // Consume only the release click from a completed hold, never the next deliberate tap.
  document.addEventListener('pointerdown',event=>{suppressReleaseClick=false;if(actionTarget&&!actions.contains(event.target))closeActions();},{capture:true});
  document.addEventListener('click',event=>{if(suppressReleaseClick&&event.detail){event.preventDefault();event.stopImmediatePropagation();suppressReleaseClick=false;}},{capture:true});
  document.addEventListener('pointermove',event=>{if(press&&event.pointerId===press.id&&Math.hypot(event.clientX-press.x,event.clientY-press.y)>10)cancelPress();},{passive:true});
  document.addEventListener('pointerup',cancelPress,{passive:true});
  document.addEventListener('pointercancel',cancelPress,{passive:true});
  function resetActions(){cancelPress();clearHover();closeActions();}
  hoverControls.addEventListener('change',resetActions);
  window.addEventListener('resize',resetActions);window.visualViewport?.addEventListener('resize',resetActions);
  window.addEventListener('scroll',resetActions,{passive:true});window.visualViewport?.addEventListener('scroll',resetActions);
  window.addEventListener('blur',resetActions);
  document.addEventListener('visibilitychange',()=>{if(document.hidden)resetActions();});
  function position(latest=false) {
    if(!isOpen())return;
    const stick=bottom(), height=log.clientHeight, widthBefore=log.clientWidth, viewport=window.visualViewport;
    const viewTop=viewport?.offsetTop||0, viewBottom=viewTop+(viewport?.height||innerHeight);
    root.toggleAttribute('data-short',compact()&&viewBottom-viewTop<400);
    // Anchor to the row, not an avatar's animated position.
    const anchor=presence.getBoundingClientRect();
    const width=Math.min(437,document.documentElement.clientWidth-24);
    const dock=chatDock();
    const clientWidth=document.documentElement.clientWidth;
    let left,top;
    if(!dock){
      left=Math.max(12,Math.min(anchor.left-16,clientWidth-width-12));
      top=compact()?viewTop+12:Math.max(viewTop+12,Math.min(anchor.bottom+18,viewBottom-260));
    }else{
      const gap=dock.gap;
      if(dock.anchor==='right')left=Math.max(12,Math.min(anchor.right+gap,clientWidth-width-12));
      else if(dock.anchor==='left')left=Math.max(12,Math.min(anchor.left-gap-width,clientWidth-width-12));
      else left=Math.max(12,Math.min(anchor.left-16,clientWidth-width-12));
      if(compact())top=viewTop+12;
      else if(dock.anchor==='above'){
        const lifted=Math.min(672,Math.max(0,anchor.top-gap-(viewTop+12)));
        top=Math.max(viewTop+12,anchor.top-gap-lifted);
      }else if(dock.anchor==='left'||dock.anchor==='right')top=Math.max(viewTop+12,Math.min(anchor.top-8,viewBottom-260));
      else top=Math.max(viewTop+12,Math.min(anchor.bottom+gap,viewBottom-260));
      if(root.dataset.chatAnchor!==dock.anchor)root.dataset.chatAnchor=dock.anchor;
    }
    if(!dock&&root.hasAttribute('data-chat-anchor'))root.removeAttribute('data-chat-anchor');
    const panelHeight=Math.min(672,viewBottom-top-12);
    const signature=JSON.stringify([left,top,panelHeight]);
    if(signature!==positionSignature){
      positionSignature=signature;root.style.left=left+'px';root.style.top=top+'px';
      root.style.setProperty('--chat-height',panelHeight+'px');
    }
    if(latest||stick&&(log.clientHeight!==height||log.clientWidth!==widthBefore))log.scrollTop=log.scrollHeight;
    const bounds=log.getBoundingClientRect(),viewStart=bounds.top+log.clientTop,viewEnd=viewStart+log.clientHeight;
    const visible=new Map(),ahead=new Map();
    // Prepare about half a second of travel, bounded to one screen of history.
    const lookahead=Math.min(log.clientHeight,Math.max(log.clientHeight*.4,scrollSpeed*500));
    for(const message of log.querySelectorAll('.chat-message')){
      const rect=message.getBoundingClientRect(),id=message.dataset.authorId;
      if(!rect.height)continue;
      if(rect.bottom>viewStart&&rect.top<viewEnd)visible.set(id,(visible.get(id)||0)+1);
      else{
        const distance=scrollDirection>0?rect.top-viewEnd:scrollDirection<0?viewStart-rect.bottom:Infinity;
        if(distance>=0&&distance<lookahead)ahead.set(id,Math.min(ahead.get(id)??Infinity,distance));
      }
    }
    const upcoming=[...ahead].filter(([id])=>!visible.has(id)).sort((a,b)=>a[1]-b[1]).slice(0,3).map(([id])=>id);
    const context=!dock
      ?{visible:[...visible.keys()].sort(),upcoming,left:compact()?null:left+12,right:compact()?null:left+width-12}
      :{visible:[...visible.keys()].sort(),upcoming,axis:dock.stack,
        left:dock.stack==='row'&&!compact()?left+12:null,right:dock.stack==='row'&&!compact()?left+width-12:null,
        top:dock.stack==='column'&&!compact()?top+12:null,bottom:dock.stack==='column'&&!compact()?top+panelHeight-12:null};
    const contextSignature=JSON.stringify(context);
    if(contextSignature!==speakerSignature){speakerSignature=contextSignature;onSpeakerContext(context);}
    const existing=new Map([...$('pointers').children].map(pointer=>[pointer.dataset.playerId,pointer]));
    let moving=false;
    for(const badge of compact()?[]:presence.querySelectorAll('[data-player-id]')){
      const animation=badge.getAnimations().find(animation=>animation.id==='presence-reorder'&&animation.playState!=='finished');
      if(animation)moving=true;
      // At 65% of this ease-in/out movement, the portrait is ~90% home and slowing.
      const settling=!animation||Number(animation.currentTime)/Number(animation.effect.getTiming().duration)>=.65;
      const rect=badge.getBoundingClientRect(),centerX=rect.left+rect.width/2,centerY=rect.top+rect.height/2;
      if(!visible.has(badge.dataset.playerId)||!settling||rect.top<viewTop)continue;
      // One visible message is 85%, three is neutral, and five or more is 115%.
      // Vary only the cross-axis so both the portrait tip and panel seam stay attached.
      const scale=(.85+.075*Math.min(4,visible.get(badge.dataset.playerId)-1)).toFixed(3);
      let cross,tail,side='below';
      if(!dock){
        if(rect.bottom+4>=top||centerX<left+12||centerX>left+width-12)continue;
        cross=(centerX-left)+'px';tail=(top-rect.bottom-4)+'px';
      }else if(dock.anchor==='right'||dock.anchor==='left'){
        const edge=dock.anchor==='right'?left:left+width;
        const reach=dock.anchor==='right'?edge-(rect.right+4):(rect.left-4)-edge;
        if(reach<=0||centerY<top+12||centerY>top+panelHeight-12||rect.bottom>viewBottom)continue;
        side=dock.anchor;cross=(centerY-top)+'px';tail=reach+'px';
      }else if(dock.anchor==='above'){
        const edge=top+panelHeight,reach=rect.top-4-edge;
        if(reach<=0||centerX<left+12||centerX>left+width-12)continue;
        side='above';cross=(centerX-left)+'px';tail=reach+'px';
      }else continue;
      let pointer=existing.get(badge.dataset.playerId);
      if(!pointer){
        pointer=document.createElement('span');pointer.className='chat-pointer';pointer.dataset.playerId=badge.dataset.playerId;
        pointer.style.setProperty('--tail-scale',scale);
        $('pointers').append(pointer);
        if(!motion.matches){
          const from=side==='below'||side==='above'?'translateX(-50%) scaleX(var(--tail-scale)) scaleY(0)':'translateY(-50%) scaleY(var(--tail-scale)) scaleX(0)';
          const to=side==='below'||side==='above'?'translateX(-50%) scaleX(var(--tail-scale)) scaleY(1)':'translateY(-50%) scaleY(var(--tail-scale)) scaleX(1)';
          pointer.animate([{opacity:0,transform:from},{opacity:1,transform:to}],{duration:140,easing:'ease-out'});
        }
      }
      if(side==='below'){
        if(pointer.style.left!==cross)pointer.style.left=cross;
      }else{
        if(pointer.style.left)pointer.style.left='';
        if(pointer.style.getPropertyValue('--tail-cross')!==cross)pointer.style.setProperty('--tail-cross',cross);
      }
      if(pointer.style.getPropertyValue('--tail-height')!==tail)pointer.style.setProperty('--tail-height',tail);
      if(pointer.style.getPropertyValue('--tail-scale')!==scale)pointer.style.setProperty('--tail-scale',scale);
      existing.delete(badge.dataset.playerId);
    }
    for(const pointer of existing.values())pointer.remove();
    // Only run frames while a finite avatar movement is in flight; idle chat sleeps.
    if(moving)schedulePosition();
  }
  // Follow badge replacement, theme changes, wrapping and the mobile keyboard.
  const resize=new ResizeObserver(schedulePosition);resize.observe(document.querySelector('body > header'));resize.observe(log);
  new MutationObserver(schedulePosition).observe(presence,{childList:true});
  new MutationObserver(schedulePosition).observe(document.documentElement,{attributes:true,attributeFilter:['data-theme']});
  window.addEventListener('infuser-presence-motion',schedulePosition);
  motion.addEventListener('change',schedulePosition);
  window.addEventListener('resize',schedulePosition);window.addEventListener('scroll',schedulePosition,{passive:true});
  window.visualViewport?.addEventListener('resize',schedulePosition);window.visualViewport?.addEventListener('scroll',schedulePosition);
  const markRead=()=>{lastRead=Math.max(lastRead,...messages.map(m=>m.id));setHidden($('new'),true);updateToggle();};
  function updateToggle() {
    const unread=messages.filter(m=>m.id>lastRead&&m.author.id!==me.player_id).length;
    setText($('unread'),String(unread));setHidden($('unread'),!unread);
    const label=(isOpen()?'Hide chat':'Show chat')+(unread?`, ${unread} unread messages`:'');
    if($('toggle').getAttribute('aria-label')!==label)$('toggle').setAttribute('aria-label',label);
    $('toggle').classList.toggle('has-unread',unread>0);
    // This runs during scrolling. Updating the unread badge must never scroll the log.
  }
  function scrollLatest(){log.scrollTop=log.scrollHeight;lastScrollTop=log.scrollTop;scrollDirection=0;scrollSpeed=0;markRead();schedulePosition();}
  function setOpen(open,restoreFocus=true) {
    root.hidden=!open;$('panel').hidden=!open;$('toggle').setAttribute('aria-expanded',String(open));$('toggle').title=open?'Hide chat':'Show chat';
    updateToggle();
    if(open){scrollDirection=0;scrollSpeed=0;position(true);scrollLatest();schedulePosition();(me.inspecting||compact()?log:input).focus({preventScroll:true});poll();}
    else{
      emojiPicker.close();
      resetActions();
      cancelAnimationFrame(positionFrame);positionFrame=null;speakerSignature=null;scrollDirection=0;scrollSpeed=0;
      $('pointers').replaceChildren();onSpeakerContext(null);
      if(restoreFocus)$('toggle').focus({preventScroll:true});
    }
  }
  $('toggle').onclick=()=>setOpen(!isOpen());$('close').onclick=()=>setOpen(false);
  root.addEventListener('keydown',event=>{if(event.key==='Escape'&&isOpen()){event.preventDefault();event.stopPropagation();setOpen(false);}});
  $('toggle').addEventListener('keydown',event=>{if(event.key==='Escape'&&isOpen()){event.preventDefault();setOpen(false);}});
  document.addEventListener('pointerdown',event=>{if(isOpen()&&!root.contains(event.target)&&!$('toggle').contains(event.target)&&!event.target.closest('.screenshot-viewer'))setOpen(false,false);});
  $('new').onclick=scrollLatest;
  log.addEventListener('scroll',()=>{
    resetActions();
    const now=performance.now(),delta=log.scrollTop-lastScrollTop;
    if(delta){
      const direction=Math.sign(delta),speed=Math.abs(delta)/Math.max(16,Math.min(200,now-lastScrollAt));
      scrollSpeed=direction===scrollDirection ? .5*scrollSpeed+.5*speed : speed;scrollDirection=direction;
    }
    lastScrollTop=log.scrollTop;lastScrollAt=now;
    if(bottom()&&isOpen())markRead();schedulePosition();considerRead();
  },{passive:true});
  // Keep the native textarea's direction, caret scrolling and selection intact.
  // A separate native scrollport exposes the same vertical range on the left.
  const scrollbar=$('scrollbar');
  let mirroredScrollTop=0;
  function syncInputScroll() {
    if(!input.clientHeight)return;
    setHidden(scrollbar,input.scrollHeight<=input.clientHeight);
    const height=input.scrollHeight-input.clientHeight+scrollbar.clientHeight+'px';
    if($('scroll-range').style.height!==height)$('scroll-range').style.height=height;
    if(scrollbar.scrollTop!==input.scrollTop)scrollbar.scrollTop=input.scrollTop;
    mirroredScrollTop=scrollbar.scrollTop;
  }
  input.addEventListener('scroll',syncInputScroll,{passive:true});
  scrollbar.addEventListener('scroll',()=>{
    // Ignore queued events from our own writes: they must not undo a newer
    // native caret movement before the textarea's scroll event is delivered.
    if(scrollbar.scrollTop===mirroredScrollTop)return;
    mirroredScrollTop=scrollbar.scrollTop;
    input.scrollTop=mirroredScrollTop;
  },{passive:true});
  new ResizeObserver(syncInputScroll).observe(input);
  input.oninput=()=>{
    syncInputScroll();
    const length=Array.from(input.value).length;
    setText($('limit'),length+'/280');setHidden($('limit'),length<240);
    $('limit').classList.toggle('at-limit',length>=280);
    $('send').disabled=sending||(!input.value.trim()&&!screenshot);
    $('emoji-toggle').disabled=sending;
  };
  input.addEventListener('keydown',event=>{
    if(event.key==='Enter'&&!event.shiftKey&&!event.isComposing){event.preventDefault();if(!$('send').disabled)$('form').requestSubmit();}
  });
  function row(message) {
    const own=message.author.id===me.player_id, article=document.createElement('article');
    article.className='chat-message'+(own?' own':'');article.dataset.id=message.id;article.dataset.authorId=message.author.id;
    const portrait=profileAvatar(message.author.name,message.author.id,message.author.avatar);
    portrait.classList.add('chat-portrait');portrait.title=message.author.name;
    const bubble=document.createElement('div');bubble.className='chat-bubble';
    const meta=document.createElement('div');meta.className='chat-meta';
    const name=document.createElement('strong');name.textContent=message.author.name;
    const time=document.createElement('time');time.dateTime=new Date(message.created*1000).toISOString();time.textContent=new Date(message.created*1000).toLocaleTimeString([],{hour:'numeric',minute:'2-digit'});time.title=new Date(message.created*1000).toLocaleString();
    meta.append(name,time);
    if(message.can_delete){
      bubble.tabIndex=0;bubble.classList.add('has-actions');bubble.setAttribute('role','group');bubble.setAttribute('aria-haspopup','menu');bubble.setAttribute('aria-keyshortcuts','Shift+F10');bubble.setAttribute('aria-label','Message from '+message.author.name);
      const remove=document.createElement('button');remove.type='button';remove.className='chat-remove';remove.textContent='×';remove.title='Delete message';remove.disabled=true;remove.setAttribute('aria-label','Delete message from '+message.author.name);
      remove.onclick=()=>{if(hoverAction?.bubble===bubble&&performance.now()>=hoverAction.readyAt)deleteMessage(message);};
      bubble.addEventListener('pointerenter',event=>{if(event.pointerType==='mouse')revealDelete(bubble,remove);});
      bubble.addEventListener('pointerleave',()=>{if(hoverAction?.bubble===bubble&&!bubble.matches(':focus-visible')&&!remove.matches(':focus-visible'))clearHover();});
      bubble.addEventListener('focusin',()=>{if(bubble.matches(':focus-visible')||remove.matches(':focus-visible'))revealDelete(bubble,remove);});
      bubble.addEventListener('focusout',event=>{if(hoverAction?.bubble===bubble&&!bubble.contains(event.relatedTarget))clearHover();});
      bubble.addEventListener('keydown',event=>{
        if(event.key==='ContextMenu'||event.key==='F10'&&event.shiftKey){event.preventDefault();showActions(message,bubble);}
      });
      bubble.addEventListener('pointerdown',event=>{
        if(event.button!==0||!event.isPrimary||deleteBusy)return;
        if(event.pointerType==='mouse'&&hoverControls.matches)return;
        cancelPress();const state={id:event.pointerId,x:event.clientX,y:event.clientY,bubble};press=state;
        state.timer=setTimeout(()=>{if(press===state&&bubble.isConnected){suppressReleaseClick=true;showActions(message,bubble);}},550);
      });
      bubble.addEventListener('contextmenu',event=>{event.preventDefault();cancelPress();suppressReleaseClick=true;showActions(message,bubble);});
      meta.append(remove);
    }
    bubble.append(meta,messageText(message.text));if(message.screenshot)bubble.append(screenshotTile({id:message.screenshot,api:request,context:{chat:message.id},onLoad:schedulePosition}));article.append(portrait,bubble);return article;
  }
  function render(next) {
    const stick=bottom(), previousIds=new Set(messages.map(m=>m.id));
    const added=initialized?next.filter(m=>!previousIds.has(m.id)&&m.author.id!==me.player_id):[];
    const nodes=new Map([...log.querySelectorAll('.chat-message')].map(n=>[Number(n.dataset.id),n]));
    let changed=next.length!==messages.length;
    // Keep unchanged nodes so polling doesn't reset selection, focus or scroll position.
    for(const message of next){
      const signature=JSON.stringify(message);let node=nodes.get(message.id);
      if(node?.dataset.signature===signature){nodes.delete(message.id);continue;}
      changed=true;
      const replacement=row(message);replacement.dataset.signature=signature;
      if(node){node.replaceWith(replacement);nodes.delete(message.id);}else log.append(replacement);
    }
    for(const node of nodes.values())node.remove();
    if(next.length)log.querySelector('.chat-empty')?.remove();
    if(!next.length&&!log.querySelector('.chat-empty')){const empty=document.createElement('p');empty.className='chat-empty';empty.textContent=emptyMessage;log.append(empty);}
    messages=next;
    if(hoverAction&&!hoverAction.bubble.isConnected)clearHover();
    if(press&&!press.bubble.isConnected)cancelPress();
    if(actionTarget&&(!actionTarget.bubble.isConnected||!messages.some(m=>m.id===actionTarget.message.id&&m.can_delete)))closeActions(true);
    if(!initialized){lastRead=Math.max(0,...next.map(m=>m.id));initialized=true;}
    if(isOpen()&&stick&&changed&&!document.hidden)scrollLatest();
    else if(added.length&&isOpen())$('new').hidden=false;
    if(added.length&&isOpen())$('announcement').textContent=added.length===1?`${added[0].author.name}: ${added[0].text}`:`${added.length} new messages`;
    updateToggle();
    position();
    paintReads();
    considerRead();
  }
  function paintReads() {
    const stick=bottom();
    const rows=[...log.querySelectorAll('.chat-message')].map(node=>({id:Number(node.dataset.id),node}));
    const buckets=placeReads(rows.map(row=>row.id),reads);
    let changed=false;
    for(const row of rows){
      const people=buckets.get(row.id)||[];
      const existing=row.node.querySelector('.chat-reads');
      const signature=people.map(person=>person.id+':'+person.message).join(',');
      if(!people.length){if(existing){existing.remove();changed=true;}continue;}
      if(existing?.dataset.reads===signature)continue;
      const box=document.createElement('div');box.className='chat-reads';box.dataset.reads=signature;
      for(const person of people){
        const mark=document.createElement('span');mark.className='chat-read';mark.title=person.name;
        mark.setAttribute('role','img');mark.setAttribute('aria-label',person.name+', read up to here');
        const face=profileAvatar(person.name,person.id,person.avatar);if(face.tagName==='IMG')face.alt='';
        mark.append(face);box.append(mark);
      }
      existing?existing.replaceWith(box):row.node.append(box);
      changed=true;
    }
    if(changed&&isOpen()&&stick&&!document.hidden)scrollLatest();
  }
  function considerRead() {
    if(readHold||readFlight||me.inspecting)return;
    const active=isOpen()&&!document.hidden;
    const bounds=log.getBoundingClientRect();
    const boxes=[...log.querySelectorAll('.chat-message')].map(node=>{
      const rect=node.getBoundingClientRect();
      return {id:Number(node.dataset.id),top:rect.top,bottom:rect.bottom,height:rect.height};
    });
    const seen=seenLimit(boxes,bounds.top+log.clientTop,bounds.top+log.clientTop+log.clientHeight);
    const next=advancedRead(readAck,seen,active);
    if(next===readAck)return;
    const sent=next;readFlight=sent;
    request('/api/chat/read',{method:'POST',body:JSON.stringify({message:sent})})
      .then(body=>{
        const stored=Number(body?.message)||sent;
        readAck=Math.max(readAck,stored);
        if(me.player_id){
          const mine=reads.find(read=>read&&read.id===me.player_id);
          if(mine)mine.message=Math.max(mine.message,readAck);
          else reads=[...reads,{id:me.player_id,name:me.name||'',avatar:me.avatar??null,message:readAck}];
          paintReads();
        }
      })
      .catch(()=>{readHold=true;})
      .finally(()=>{if(readFlight===sent)readFlight=0;});
  }
  async function poll() {
    clearTimeout(timer);
    if(busy||document.hidden)return;
    busy=true;
    try{
      const data=await request('/api/chat');
      reads=data.reads||[];
      const ownRead=reads.find(read=>read&&read.id===me.player_id);
      if(ownRead)readAck=Math.max(readAck,Number(ownRead.message)||0);
      readHold=false;
      render(data.messages);
      setText($('connection'),'');
    }
    catch(error){$('connection').textContent=error.status===401||error.status===403?'Sign in again to chat.':'Reconnecting…';}
    finally{busy=false;timer=setTimeout(poll,isOpen()?3000:10000);}
  }
  $('form').onsubmit=async event=>{
    event.preventDefault();if(sending||(!input.value.trim()&&!screenshot))return;
    const text=input.value.trim();
    if(!retry||retry.text!==text||retry.screenshot!==screenshot)retry={text,screenshot,client_id:crypto.randomUUID()};
    emojiPicker.close();
    sending=true;root.dataset.sending='true';input.readOnly=true;input.oninput();$('error').textContent='';$('send').setAttribute('aria-label','Sending message');$('send').setAttribute('aria-busy','true');
    try{
      await request('/api/chat',{method:'POST',body:JSON.stringify(retry)});
      input.value='';retry=null;screenshot=null;attachmentDraft(draft,null,request,()=>{});
      // Wait for an existing read to finish before the immediate post-send read.
      while(busy)await new Promise(resolve=>setTimeout(resolve,30));
      await poll();if(isOpen())scrollLatest();
    }catch(error){$('error').textContent=error.message;}
    finally{sending=false;delete root.dataset.sending;input.readOnly=false;$('send').setAttribute('aria-label','Send message');$('send').removeAttribute('aria-busy');input.oninput();if(isOpen())input.focus();}
  };
  document.addEventListener('visibilitychange',()=>{if(document.hidden)clearTimeout(timer);else poll();});
  window.addEventListener('online',poll);
  function consumeChatLink(open){
    if(location.hash!=='#chat')return;
    const url=new URL(location.href);
    url.hash='';
    url.searchParams.delete('chat');
    history.replaceState(history.state,'',url);
    if(open)setOpen(true,false);
  }
  window.addEventListener('hashchange',()=>consumeChatLink(true));
  navigator.serviceWorker?.addEventListener('message',event=>{if(event.data?.type==='infuser-open-chat'){if(me.inspecting)openHomeChat();else setOpen(true,false);}});
  // Older notification links can still be in the address bar at refresh time.
  consumeChatLink(performance.getEntriesByType('navigation')[0]?.type!=='reload');
  poll();
}
