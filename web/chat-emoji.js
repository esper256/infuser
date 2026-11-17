// Lazy, local-only picker. Selection is inserted into the draft, never sent.
export function initChatEmoji({root, input, button, panel}) {
  const status=panel.querySelector('[role="status"]'), mount=panel.querySelector('.chat-emoji-mount');
  let picker, loading, start=0, end=0;
  const open=()=>!panel.hidden;
  function remember(){start=input.selectionStart;end=input.selectionEnd;}
  for(const event of ['select','keyup','click','input'])input.addEventListener(event,remember);
  function close(focus=false){
    panel.hidden=true;button.setAttribute('aria-expanded','false');
    if(focus)button.focus({preventScroll:true});
  }
  async function load(){
    if(picker)return picker;
    if(!loading)loading=(async()=>{
      const {default:Picker}=await import('/assets/vendor/emoji-picker/picker.js');
      const candidate=new Picker({dataSource:'/assets/vendor/emoji-picker/data.json'});
      candidate.classList.add('dark');
      mount.replaceChildren(candidate);
      try{await candidate.database.ready();}
      catch(error){candidate.remove();throw error;}
      candidate.addEventListener('emoji-click',event=>{
        if(input.readOnly||input.disabled||!open())return;
        const emoji=event.detail.unicode;
        if(!emoji)return;
        const value=input.value.slice(0,start)+emoji+input.value.slice(end);
        // Preserve the textarea's UTF-16 maxlength as well as the server's character limit.
        if(value.length>input.maxLength){
          status.textContent='No room for that emoji. Shorten your message first.';return;
        }
        input.setSelectionRange(start,end);
        input.setRangeText(emoji,start,end,'end');
        input.dispatchEvent(new Event('input',{bubbles:true}));
        remember();close();input.focus({preventScroll:true});
      });
      picker=candidate;return picker;
    })().finally(()=>{loading=null;});
    return loading;
  }
  button.addEventListener('click',async()=>{
    if(open()){close(true);return;}
    if(input.readOnly||input.disabled)return;
    remember();panel.hidden=false;button.setAttribute('aria-expanded','true');
    status.textContent=picker?'':'Loading emojis…';
    panel.querySelector('.chat-emoji-close').focus({preventScroll:true});
    try{
      const ready=await load();
      status.textContent='';
      if(open())ready.shadowRoot.querySelector('input[type="search"]')?.focus({preventScroll:true});
    }catch{
      status.textContent='Could not load emojis. Close and try again.';
    }
  });
  panel.querySelector('.chat-emoji-close').onclick=()=>close(true);
  // Close this layer before Escape reaches the surrounding chat.
  panel.addEventListener('keydown',event=>{
    if(event.key==='Escape'){event.preventDefault();event.stopPropagation();close(true);}
  },true);
  document.addEventListener('pointerdown',event=>{
    if(open()&&!panel.contains(event.target)&&!button.contains(event.target))close();
  });
  root.addEventListener('focusout',()=>{
    queueMicrotask(()=>{if(open()&&!panel.contains(document.activeElement)&&document.activeElement!==button)close();});
  });
  return {close};
}
