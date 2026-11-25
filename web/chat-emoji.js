// Lazy local emoji picker; screenshots attach to the draft, house calls send immediately.
import { fillScreenshotPicker, screenshotTile } from '/assets/screenshots.js';
export function initChatEmoji({root, input, button, panel, api, onScreenshot, onHouseCall}) {
  const status=panel.querySelector('[role="status"]'), mount=panel.querySelector('.chat-emoji-mount');
  const tabs=document.createElement('div');tabs.className='attachment-tabs';tabs.setAttribute('role','tablist');tabs.setAttribute('aria-label','Attachments');
  const shots=document.createElement('div'),calls=document.createElement('div');
  for(const pane of [shots,calls]){pane.className='screenshot-picker';pane.hidden=true;panel.append(pane);}
  const panes=[mount,shots,calls],buttons=[];
  for(const [index,label] of ['Emoji','Screenshots','House calls'].entries()){
    const target=panes[index],tab=document.createElement('button');target.id=panel.id+'-tab-'+index;target.setAttribute('role','tabpanel');
    tab.type='button';tab.textContent=label;tab.id=target.id+'-button';tab.setAttribute('role','tab');tab.setAttribute('aria-controls',target.id);target.setAttribute('aria-labelledby',tab.id);tabs.append(tab);buttons.push(tab);
    tab.onclick=()=>selectTab(index);
  }
  const callTab=buttons[2];callTab.hidden=true;
  let available=[],availabilityRequest=0;
  panel.querySelector('.chat-emoji-heading strong').replaceWith(tabs);
  const choose=callback=>id=>{if(input.disabled||input.readOnly)return;callback?.(id);close();input.focus({preventScroll:true});};
  function selectTab(index){
    panes.forEach((pane,i)=>{pane.hidden=i!==index;buttons[i].setAttribute('aria-selected',String(i===index));buttons[i].tabIndex=i===index?0:-1;});status.textContent='';
    if(index===1)void fillScreenshotPicker(shots,api,choose(onScreenshot));
    if(index===2){
      const grid=document.createElement('div');grid.className='screenshot-grid';calls.replaceChildren(grid);
      for(const call of available)grid.append(screenshotTile({id:call.id,houseCall:call,pick:choose(onHouseCall)}));
    }
  }
  async function loadHouseCalls(){
    const request=++availabilityRequest;callTab.hidden=true;available=[];
    try{const data=await api('/api/house-calls');if(request!==availabilityRequest)return;available=data.images||[];callTab.hidden=!available.length;}catch{/* Unavailable house calls leave the other attachment tabs usable. */}
  }
  tabs.onkeydown=event=>{if(['ArrowLeft','ArrowRight','Home','End'].includes(event.key)){
    event.preventDefault();const visible=buttons.filter(tab=>!tab.hidden),index=visible.indexOf(document.activeElement);
    const next=event.key==='Home'?visible[0]:event.key==='End'?visible.at(-1):visible[(index+(event.key==='ArrowRight'?1:visible.length-1))%visible.length];next.click();next.focus();
  }};
  selectTab(0);
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
    selectTab(0);void loadHouseCalls();
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
