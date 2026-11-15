// Shared Library room. All message text stays text, never markup or links.
export function initChat({me, api, profileAvatar}) {
  const request=(path,options={})=>api(path,{...options,signal:AbortSignal.timeout(12000)});
  const root=document.createElement('aside');root.id='library-chat';root.setAttribute('aria-label','Library chat');
  root.innerHTML=`<button id="chat-toggle" type="button" aria-expanded="false" aria-controls="chat-panel"><span class="chat-icon" aria-hidden="true">···</span><span>Chat</span><span id="chat-faces" aria-hidden="true"></span><span id="chat-unread" hidden></span></button>
    <section id="chat-panel" aria-labelledby="chat-title" hidden>
      <div class="chat-heading"><div><h2 id="chat-title">Chat</h2><span class="chat-audience">Everyone in the Library</span></div><button id="chat-close" type="button" aria-label="Close chat">×</button></div>
      <div id="chat-log" role="log" aria-label="Messages" aria-live="off" tabindex="0"><p class="chat-empty">No messages yet.</p></div>
      <button id="chat-new" class="chat-new" type="button" hidden>New messages ↓</button>
      <p id="chat-connection" role="status"></p><p id="chat-error" role="alert"></p>
      <form id="chat-form"><label class="sr-only" for="chat-text">Message everyone</label><textarea id="chat-text" maxlength="280" rows="2" placeholder="Message…" aria-describedby="chat-limit"></textarea><div class="chat-compose-actions"><span id="chat-limit">0 / 280</span><button id="chat-send" type="submit" disabled>Send</button></div></form>
      <span id="chat-announcement" class="sr-only" role="status"></span>
    </section>`;
  document.body.append(root);
  const $=id=>root.querySelector('#chat-'+id), log=$('log'), input=$('text');
  let messages=[], initialized=false, busy=false, sending=false, timer, retry, lastRead=0;
  const bottom=()=>log.scrollHeight-log.scrollTop-log.clientHeight<32;
  const isOpen=()=>!$('panel').hidden;
  // Mobile keyboards resize the visual viewport, not always the layout viewport.
  if(window.visualViewport){
    const fit=()=>{
      const stick=isOpen()&&bottom();
      root.style.setProperty('--chat-viewport',window.visualViewport.height+'px');
      root.style.setProperty('--chat-keyboard',Math.max(0,innerHeight-window.visualViewport.height-window.visualViewport.offsetTop)+'px');
      if(stick)requestAnimationFrame(()=>{log.scrollTop=log.scrollHeight;});
    };
    window.visualViewport.addEventListener('resize',fit);window.visualViewport.addEventListener('scroll',fit);fit();
  }
  const markRead=()=>{lastRead=Math.max(lastRead,...messages.map(m=>m.id));$('new').hidden=true;updateToggle();};
  function updateToggle() {
    const unread=messages.filter(m=>m.id>lastRead&&m.author.id!==me.player_id).length;
    $('unread').textContent=unread>9?'9+':String(unread);$('unread').hidden=!unread;
    $('toggle').setAttribute('aria-label',unread?`Chat, ${unread} unread messages`:'Chat');
    $('toggle').classList.toggle('has-unread',unread>0);
    const people=[...new Map([...messages].reverse().map(m=>[m.author.id,m.author])).values()].slice(0,3);
    $('faces').replaceChildren(...people.map(p=>profileAvatar(p.name,p.id,p.avatar)));
  }
  function scrollLatest(){log.scrollTop=log.scrollHeight;markRead();}
  function setOpen(open) {
    $('panel').hidden=!open;$('toggle').setAttribute('aria-expanded',String(open));
    if(open){scrollLatest();input.focus();poll();}else $('toggle').focus();
  }
  $('toggle').onclick=()=>setOpen(!isOpen());$('close').onclick=()=>setOpen(false);
  root.addEventListener('keydown',event=>{if(event.key==='Escape'&&isOpen()){event.preventDefault();event.stopPropagation();setOpen(false);}});
  $('new').onclick=scrollLatest;
  log.addEventListener('scroll',()=>{if(bottom()&&isOpen())markRead();},{passive:true});
  input.oninput=()=>{
    $('limit').textContent=Array.from(input.value).length+' / 280';
    $('send').disabled=sending||!input.value.trim();
  };
  input.addEventListener('keydown',event=>{
    if(event.key==='Enter'&&!event.shiftKey&&!event.isComposing){event.preventDefault();if(!$('send').disabled)$('form').requestSubmit();}
  });
  function row(message) {
    const own=message.author.id===me.player_id, article=document.createElement('article');
    article.className='chat-message'+(own?' own':'');article.dataset.id=message.id;
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
    // Keep unchanged nodes so polling doesn't reset selection, focus or scroll position.
    for(const message of next){
      const signature=JSON.stringify(message);let node=nodes.get(message.id);
      if(node?.dataset.signature===signature){nodes.delete(message.id);continue;}
      const replacement=row(message);replacement.dataset.signature=signature;
      if(node){node.replaceWith(replacement);nodes.delete(message.id);}else log.append(replacement);
    }
    for(const node of nodes.values())node.remove();
    log.querySelector('.chat-empty')?.remove();
    if(!next.length){const empty=document.createElement('p');empty.className='chat-empty';empty.textContent='No messages yet.';log.append(empty);}
    messages=next;
    if(!initialized){lastRead=Math.max(0,...next.map(m=>m.id));initialized=true;}
    if(isOpen()&&stick&&!document.hidden)scrollLatest();
    else if(added.length&&isOpen())$('new').hidden=false;
    if(added.length&&isOpen())$('announcement').textContent=added.length===1?`${added[0].author.name}: ${added[0].text}`:`${added.length} new messages`;
    updateToggle();
  }
  async function poll() {
    clearTimeout(timer);
    if(busy||document.hidden)return;
    busy=true;
    try{const data=await request('/api/chat');render(data.messages);$('connection').textContent='';}
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
  poll();
}
