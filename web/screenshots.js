// All reads use the page's pinned profile/visit API. Image data has no public URL.
export const attachmentIcon='<svg viewBox="0 0 28 28" aria-hidden="true" focusable="false"><rect x="11" y="2" width="15" height="13" rx="2"/><path d="m12 13 5-6 4 4 3-2"/><circle cx="11" cy="16" r="10" class="attachment-face"/><path d="M6 18c1 5 9 5 10 0M7 13h.01M15 13h.01"/></svg>';
const node=(tag,text)=>{const n=document.createElement(tag);if(text)n.textContent=text;return n;};
const path=(id,context={},thumbnail=false)=>'/api/screenshots/'+encodeURIComponent(id)+'?'+new URLSearchParams({...context,thumbnail:String(thumbnail)});
let viewer;
export function screenshotTile({id,api,context={},pick,onLoad=()=>{}}){
  const button=node('button');button.type='button';button.className='screenshot-tile';button.setAttribute('aria-label',pick?'Attach screenshot':'Open screenshot');
  const image=node('img');image.alt='Game screenshot';image.hidden=true;button.append(image);
  let available=false;
  api(path(id,context,true)).then(data=>{if(!button.isConnected)return;image.onload=()=>{image.hidden=false;onLoad();};image.src=data.image;available=true;}).catch(error=>{button.replaceChildren(node('span',error.status===404?'Screenshot expired':'Screenshot unavailable'));button.disabled=true;onLoad();});
  button.onclick=()=>{if(available){if(pick)pick(id);else showScreenshot({id,api,context});}};
  return button;
}
async function showScreenshot({id,api,context}){
  if(!viewer){
    viewer=node('dialog');viewer.className='screenshot-viewer';viewer.setAttribute('aria-label','Screenshot');
    const close=node('button','×');close.type='button';close.className='close';close.setAttribute('aria-label','Close screenshot');close.onclick=()=>viewer.close();
    viewer.append(close,node('div'));document.body.append(viewer);
    viewer.addEventListener('click',event=>{if(event.target===viewer)viewer.close();});
    viewer.addEventListener('close',()=>{viewer.dataset.request='';viewer.lastChild.replaceChildren();});
  }
  const request=crypto.randomUUID();viewer.dataset.request=request;viewer.lastChild.replaceChildren(node('p','Loading screenshot…'));if(!viewer.open)viewer.showModal();
  try{const data=await api(path(id,context));if(viewer.dataset.request!==request)return;const image=node('img');image.alt='Game screenshot';image.src=data.image;viewer.lastChild.replaceChildren(image);}
  catch(error){if(viewer.dataset.request===request)viewer.lastChild.replaceChildren(node('p',error.status===404?'Screenshot expired.':error.message));}
}
export async function fillScreenshotPicker(container,api,pick){
  const request=crypto.randomUUID();container.dataset.request=request;container.replaceChildren(node('p','Loading screenshots…'));
  try{
    const data=await api('/api/screenshots');if(container.dataset.request!==request)return;
    container.replaceChildren();
    if(!data.screenshots.length){container.append(node('p','Take a screenshot using the camera beside Fullscreen in a web game. Your latest 10 appear here.'));return;}
    const grid=node('div');grid.className='screenshot-grid';container.append(grid);
    for(const shot of data.screenshots)grid.append(screenshotTile({id:shot.id,api,pick}));
    container.append(node('p','Latest 10 · Oldest images expire, including chat attachments.'));
  }catch(error){if(container.dataset.request===request)container.replaceChildren(node('p',error.message));}
}
export function attachmentDraft(container,id,api,remove,context={}){
  container.replaceChildren();container.hidden=!id;if(!id)return;
  container.append(screenshotTile({id,api,context}));const button=node('button','×');button.type='button';button.setAttribute('aria-label','Remove screenshot attachment');button.onclick=remove;container.append(button);
}
