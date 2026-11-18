// Lazy, local-only picker. Selection is inserted into the draft, never sent.
import { fillScreenshotPicker } from '/assets/screenshots.js';
export function initChatEmoji({root, input, button, panel, api, onScreenshot}) {
  const status=panel.querySelector('[role="status"]'), mount=panel.querySelector('.chat-emoji-mount');
  const tabs=document.createElement('div');tabs.className='attachment-tabs';tabs.setAttribute('role','tablist');tabs.setAttribute('aria-label','Emoji or screenshot');
  const shots=document.createElement('div');shots.className='screenshot-picker';shots.hidden=true;panel.append(shots);
  mount.id=panel.id+'-emojis';shots.id=panel.id+'-screenshots';mount.setAttribute('role','tabpanel');shots.setAttribute('role','tabpanel');
  const emojiTab=document.createElement('button'),shotTab=document.createElement('button');
  for(const [tab,label,target] of [[emojiTab,'Emoji',mount],[shotTab,'Screenshots',shots]]){tab.type='button';tab.textContent=label;tab.id=target.id+'-tab';tab.setAttribute('role','tab');tab.setAttribute('aria-controls',target.id);target.setAttribute('aria-labelledby',tab.id);tabs.append(tab);}
  panel.querySelector('.chat-emoji-heading strong').replaceWith(tabs);
  function selectTab(screenshots){
    mount.hidden=screenshots;shots.hidden=!screenshots;status.textContent='';
    emojiTab.setAttribute('aria-selected',String(!screenshots));shotTab.setAttribute('aria-selected',String(screenshots));emojiTab.tabIndex=screenshots?-1:0;shotTab.tabIndex=screenshots?0:-1;
    if(screenshots)void fillScreenshotPicker(shots,api,id=>{if(input.disabled||input.readOnly)return;onScreenshot?.(id);close();input.focus({preventScroll:true});});
  }
  emojiTab.onclick=()=>selectTab(false);shotTab.onclick=()=>selectTab(true);
  tabs.onkeydown=event=>{if(['ArrowLeft','ArrowRight','Home','End'].includes(event.key)){event.preventDefault();const next=event.key==='Home'?emojiTab:event.key==='End'?shotTab:document.activeElement===emojiTab?shotTab:emojiTab;next.click();next.focus();}};
  selectTab(false);
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
    selectTab(false);
    status.textContent=picker?'':'Loading emojis…';
    panel.querySelector('.chat-emoji-close').focus({preventScroll:true});
    try{
      const ready=await load();
      status.textContent='';
      if(open()&&!mount.hidden)ready.shadowRoot.querySelector('input[type="search"]')?.focus({preventScroll:true});
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
  root.addEventListener('focusout',event=>{
    if(panel.contains(event.relatedTarget)||event.relatedTarget===button)return;
    // Pointer focus can briefly be the document body between blur and focus.
    requestAnimationFrame(()=>{if(open()&&!panel.contains(document.activeElement)&&document.activeElement!==button)close();});
  });
  return {close};
}
