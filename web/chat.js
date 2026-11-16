import { openHomeChat } from '/assets/groups.js';
// Shared Library room. All message text stays text, never markup or links.
export function initChat({me, api, profileAvatar,onVisibleAuthors=()=>{}}) {
  const request=(path,options={})=>api(path,{...options,signal:AbortSignal.timeout(12000)});
  const root=document.createElement('aside');root.id='library-chat';root.setAttribute('aria-label','Library chat');
  root.innerHTML=`<button id="chat-toggle" type="button" aria-expanded="false" aria-controls="chat-panel" title="Show chat"><span class="chat-icon" aria-hidden="true">···</span><span class="sr-only">Chat</span><span id="chat-unread" hidden></span></button>
    <div id="chat-pointers" aria-hidden="true"></div>
    <section id="chat-panel" aria-labelledby="chat-title" hidden>
      <div class="chat-heading"><div><h2 id="chat-title">Chat</h2><span class="chat-audience">Everyone in the Library</span></div><button id="chat-close" type="button" aria-label="Close chat">×</button></div>
      <div id="chat-log" role="log" aria-label="Messages" aria-live="off" tabindex="0"><p class="chat-empty">No messages yet.</p></div>
      <button id="chat-new" class="chat-new" type="button" hidden>New messages ↓</button>
      <p id="chat-connection" role="status"></p><p id="chat-error" role="alert"></p>
      <form id="chat-form"><label class="sr-only" for="chat-text">Message everyone</label><textarea id="chat-text" maxlength="280" rows="2" placeholder="Message…" aria-describedby="chat-limit"></textarea><div class="chat-compose-actions"><span id="chat-limit">0 / 280</span><button id="chat-send" type="submit" disabled>Send</button></div></form>
      <span id="chat-announcement" class="sr-only" role="status"></span>
    </section>`;
  document.body.append(root);
  const $=id=>document.getElementById('chat-'+id), log=$('log'), input=$('text');
  if(me.inspecting)$('form').hidden=true;
  const presence=document.getElementById('friends'), cluster=document.createElement('div');cluster.className='presence-chat';
  presence.before(cluster);cluster.append(presence,$('toggle'));root.hidden=true;
  let messages=[], initialized=false, busy=false, sending=false, timer, retry, lastRead=0,positionFrame,positionSignature,visibleAuthorSignature;
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
  function position(latest=false) {
    if(!isOpen())return;
    const stick=bottom(), height=log.clientHeight, widthBefore=log.clientWidth, viewport=window.visualViewport;
    const viewTop=viewport?.offsetTop||0, viewBottom=viewTop+(viewport?.height||innerHeight);
    root.toggleAttribute('data-short',compact()&&viewBottom-viewTop<400);
    // Anchor to the row, not an avatar's animated position.
    const anchor=presence.getBoundingClientRect();
    const width=Math.min(437,document.documentElement.clientWidth-24);
    const left=Math.max(12,Math.min(anchor.left-16,document.documentElement.clientWidth-width-12));
    const top=compact()?viewTop+12:Math.max(viewTop+12,Math.min(anchor.bottom+18,viewBottom-260));
    const panelHeight=Math.min(672,viewBottom-top-12);
    const signature=JSON.stringify([left,top,panelHeight]);
    if(signature!==positionSignature){
      positionSignature=signature;root.style.left=left+'px';root.style.top=top+'px';
      root.style.setProperty('--chat-height',panelHeight+'px');
    }
    if(latest||stick&&(log.clientHeight!==height||log.clientWidth!==widthBefore))log.scrollTop=log.scrollHeight;
    const bounds=log.getBoundingClientRect(),visible=new Set();
    for(const message of log.querySelectorAll('.chat-message')){
      const rect=message.getBoundingClientRect();
      if(rect.height&&rect.bottom>bounds.top+log.clientTop&&rect.top<bounds.top+log.clientTop+log.clientHeight)visible.add(message.dataset.authorId);
    }
    const authors=[...visible].sort(),authorSignature=JSON.stringify(authors);
    if(authorSignature!==visibleAuthorSignature){visibleAuthorSignature=authorSignature;onVisibleAuthors(authors);}
    const existing=new Map([...$('pointers').children].map(pointer=>[pointer.dataset.playerId,pointer]));
    let moving=false;
    for(const badge of compact()?[]:presence.querySelectorAll('[data-player-id]')){
      const animation=badge.getAnimations().find(animation=>animation.id==='presence-reorder'&&animation.playState!=='finished');
      if(animation)moving=true;
      // At 65% of this ease-in/out movement, the portrait is ~90% home and slowing.
      const settling=!animation||Number(animation.currentTime)/Number(animation.effect.getTiming().duration)>=.65;
      const rect=badge.getBoundingClientRect(),center=rect.left+rect.width/2;
      if(!visible.has(badge.dataset.playerId)||!settling||rect.top<viewTop||rect.bottom+4>=top||center<left+12||center>left+width-12)continue;
      let pointer=existing.get(badge.dataset.playerId);
      if(!pointer){
        pointer=document.createElement('span');pointer.className='chat-pointer';pointer.dataset.playerId=badge.dataset.playerId;
        $('pointers').append(pointer);
        if(!motion.matches)pointer.animate([{opacity:0,transform:'translateX(-50%) scaleY(0)'},{opacity:1,transform:'translateX(-50%) scaleY(1)'}],{duration:140,easing:'ease-out'});
      }
      const x=(center-left)+'px',tail=(top-rect.bottom-4)+'px';
      if(pointer.style.left!==x)pointer.style.left=x;
      if(pointer.style.getPropertyValue('--tail-height')!==tail)pointer.style.setProperty('--tail-height',tail);
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
  function scrollLatest(){log.scrollTop=log.scrollHeight;markRead();}
  function setOpen(open,restoreFocus=true) {
    root.hidden=!open;$('panel').hidden=!open;$('toggle').setAttribute('aria-expanded',String(open));$('toggle').title=open?'Hide chat':'Show chat';
    updateToggle();
    if(open){position(true);scrollLatest();schedulePosition();(me.inspecting||compact()?log:input).focus({preventScroll:true});poll();}
    else{
      cancelAnimationFrame(positionFrame);positionFrame=null;visibleAuthorSignature=null;
      $('pointers').replaceChildren();onVisibleAuthors([]);
      if(restoreFocus)$('toggle').focus({preventScroll:true});
    }
  }
  $('toggle').onclick=()=>setOpen(!isOpen());$('close').onclick=()=>setOpen(false);
  root.addEventListener('keydown',event=>{if(event.key==='Escape'&&isOpen()){event.preventDefault();event.stopPropagation();setOpen(false);}});
  $('toggle').addEventListener('keydown',event=>{if(event.key==='Escape'&&isOpen()){event.preventDefault();setOpen(false);}});
  document.addEventListener('pointerdown',event=>{if(isOpen()&&!root.contains(event.target)&&!$('toggle').contains(event.target))setOpen(false,false);});
  $('new').onclick=scrollLatest;
  log.addEventListener('scroll',()=>{if(bottom()&&isOpen())markRead();schedulePosition();},{passive:true});
  input.oninput=()=>{
    $('limit').textContent=Array.from(input.value).length+' / 280';
    $('send').disabled=sending||!input.value.trim();
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
      const remove=document.createElement('button');remove.type='button';remove.className='chat-remove';remove.textContent='×';remove.title='Remove message';remove.setAttribute('aria-label','Remove message from '+message.author.name);
      remove.onclick=async()=>{
        remove.disabled=true;
        try{await request('/api/chat/'+message.id,{method:'DELETE'});$('error').textContent='';await poll();input.focus();}
        catch(error){$('error').textContent=error.message;remove.disabled=false;}
      };meta.append(remove);
    }
    const text=document.createElement('p');text.textContent=message.text;bubble.append(meta,text);article.append(portrait,bubble);return article;
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
    if(!next.length&&!log.querySelector('.chat-empty')){const empty=document.createElement('p');empty.className='chat-empty';empty.textContent='No messages yet.';log.append(empty);}
    messages=next;
    if(!initialized){lastRead=Math.max(0,...next.map(m=>m.id));initialized=true;}
    if(isOpen()&&stick&&changed&&!document.hidden)scrollLatest();
    else if(added.length&&isOpen())$('new').hidden=false;
    if(added.length&&isOpen())$('announcement').textContent=added.length===1?`${added[0].author.name}: ${added[0].text}`:`${added.length} new messages`;
    updateToggle();
    position();
  }
  async function poll() {
    clearTimeout(timer);
    if(busy||document.hidden)return;
    busy=true;
    try{const data=await request('/api/chat');render(data.messages);setText($('connection'),'');}
    catch(error){$('connection').textContent=error.status===401||error.status===403?'Sign in again to chat.':'Reconnecting…';}
    finally{busy=false;timer=setTimeout(poll,isOpen()?3000:10000);}
  }
  $('form').onsubmit=async event=>{
    event.preventDefault();if(sending||!input.value.trim())return;
    const text=input.value.trim();
    if(!retry||retry.text!==text)retry={text,client_id:crypto.randomUUID()};
    sending=true;root.dataset.sending='true';input.readOnly=true;input.oninput();$('error').textContent='';$('send').textContent='Sending…';
    try{
      await request('/api/chat',{method:'POST',body:JSON.stringify(retry)});
      input.value='';retry=null;
      // Wait for an existing read to finish before the immediate post-send read.
      while(busy)await new Promise(resolve=>setTimeout(resolve,30));
      await poll();if(isOpen())scrollLatest();
    }catch(error){$('error').textContent=error.message;}
    finally{sending=false;delete root.dataset.sending;input.readOnly=false;$('send').textContent='Send';input.oninput();if(isOpen())input.focus();}
  };
  document.addEventListener('visibilitychange',()=>{if(document.hidden)clearTimeout(timer);else poll();});
  window.addEventListener('online',poll);
  const openFromLink=()=>{if(location.hash==='#chat')setOpen(true,false);};
  window.addEventListener('hashchange',openFromLink);
  navigator.serviceWorker?.addEventListener('message',event=>{if(event.data?.type==='infuser-open-chat'){if(me.inspecting)openHomeChat();else setOpen(true,false);}});
  openFromLink();
  poll();
}
