// The HTML carries its own version: a delayed first request can't accidentally
// establish a newer deployment as the baseline for an older page.
export function clientUpdateAction(stamp, state) {
  if (!state) return;
  const generation = state.force_reload;
  if (Number.isSafeInteger(generation) && generation > Number(stamp?.dataset.forceReload || 0)) return 'reload';
  if (stamp?.content && typeof state.version === 'string' && state.version && state.version !== stamp.content) return 'available';
}

export function initClientUpdates({api, voiceInUse=()=>false, document:doc=document, window:win=window, markUpdate=()=>{}, now=Date.now}) {
  const stamp=doc.querySelector('meta[name="infuser-client-version"]');
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
    if(busy||reloading)return;
    busy=true;
    try {
      const current=await api('/api/client-state',{cache:'no-store',signal:AbortSignal.timeout(10000)});
      // Recheck interaction after the request: a launch or draft may have begun meanwhile.
      const action=clientUpdateAction(stamp,current);
      if(action==='reload'||(action==='available'&&current.game_open===false&&!doc.hidden&&!voiceInUse()&&!editing()&&now()-lastInteraction>=5000)){
        reloading=true;markUpdate();win.location.reload();
      }
    } catch { /* An offline or restarting server is not a reason to reload. */ }
    finally {busy=false;}
  }
  win.setInterval(check,30000);
  doc.addEventListener('visibilitychange',()=>{if(!doc.hidden)check();});
  win.addEventListener('online',check);
  check();
}
