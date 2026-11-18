// The HTML carries its own version: a delayed first request can't accidentally
// establish a newer deployment as the baseline for an older page.
export function initClientUpdates({api, document:doc=document, window:win=window, now=Date.now}) {
  const version=doc.querySelector('meta[name="infuser-client-version"]')?.content;
  if(!version)return;
  let busy=false, reloading=false, lastInteraction=now();
  for(const event of ['pointerdown','keydown','input'])doc.addEventListener(event,()=>{lastInteraction=now();},{passive:true});
  function editing() {
    // The Hidden shelf is a browsing disclosure, not an unfinished interaction.
    return doc.querySelector('dialog[open], details[open]:not(#hidden-games), [data-launching], #library-chat[data-sending]') ||
      doc.activeElement?.matches('input, textarea, select, [contenteditable="true"]') ||
      Boolean(doc.querySelector('#chat-text')?.value.trim()) ||
      Boolean(doc.querySelector('#library-chat .screenshot-draft:not([hidden])'));
  }
  async function check() {
    if(busy||reloading||doc.hidden)return;
    busy=true;
    try {
      const current=await api('/api/client-state',{cache:'no-store',signal:AbortSignal.timeout(10000)});
      // Recheck interaction after the request: a launch or draft may have begun meanwhile.
      if(typeof current.version==='string'&&current.version&&current.version!==version&&current.game_open===false&&!doc.hidden&&!editing()&&now()-lastInteraction>=5000){
        reloading=true;win.location.reload();
      }
    } catch { /* An offline or restarting server is not a reason to reload. */ }
    finally {busy=false;}
  }
  win.setInterval(check,30000);
  doc.addEventListener('visibilitychange',()=>{if(!doc.hidden)check();});
  win.addEventListener('online',check);
  check();
}
