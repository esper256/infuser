// Server-wide controls belong to the operator, never to game publishers.
export function humanWait(since, now=Date.now()/1000) {
  const seconds=Math.max(0,Math.floor(now-since));
  if(seconds<60)return 'less than a minute';
  if(seconds<3600)return Math.floor(seconds/60)+' min';
  if(seconds<86400)return Math.floor(seconds/3600)+' h '+Math.floor(seconds%3600/60)+' min';
  return Math.floor(seconds/86400)+' d '+Math.floor(seconds%86400/3600)+' h';
}
export function initServerUpdates({panel,api,me}) {
  if(!me.operator)return ()=>{};
  const node=(tag,text)=>{const n=document.createElement(tag);if(text)n.textContent=text;return n;};
  const section=node('section');section.id='server-update';section.hidden=true;panel.prepend(section);
  const dialog=panel.closest('dialog');
  const title=node('h3','Update available');title.id='server-update-title';
  section.setAttribute('aria-labelledby',title.id);
  const wait=node('p'),reason=node('p'),build=node('p'),message=node('p');message.setAttribute('role','status');
  const warning=node('p','Updating now can disconnect games and interrupt uploads or publishing in every private group. Unfinished work may need to be retried.');
  const action=node('button','Authorize one update'),cancel=node('button','Cancel'),open=node('button','Update anyway');action.type=cancel.type=open.type='button';
  const confirm=node('div');confirm.hidden=true;
  const actions=node('div');actions.className='dialog-actions';actions.append(cancel,action);
  confirm.append(warning,actions);section.append(title,wait,reason,build,open,confirm,message);
  let state,busy=false,submitting=false,requestId,reachable=true;
  function render(){
    section.hidden=!state?.available;
    if(!state)return;
    const age=state.available_since?humanWait(state.available_since):'less than a minute';
    const phase=state.report?.phase;
    const phases={checking:'Checking GitHub for the latest release',downloading:'Downloading the latest release',activating:'Switching to the update',failed:'The updater could not verify or install this release. Waiting for the next check.'};
    const stale=state.stale||Date.now()/1000-state.checked_at>=180;
    const why=!reachable?'Waiting for Infuser to respond':stale?'Waiting for a fresh update check':phases[phase]||(state.reasons?.join('. ')||'Ready for the next updater pass');
    wait.textContent=state.available?'Waiting '+age:'Infuser is up to date';
    reason.textContent=state.available?why:'';
    build.textContent=state.report?.release?'Build '+state.report.release.slice(0,12):'';
    const authorized=state.authorized_until>Date.now()/1000;
    action.disabled=submitting||!reachable||!state.available||stale||authorized||phase==='failed'||phase==='activating';
    open.disabled=action.disabled;
    open.textContent=authorized?'Update authorized':'Update anyway';
    if(authorized)message.textContent='Authorized once. The updater will recheck GitHub on its next pass, usually within a minute. Authorization expires in '+humanWait(Date.now()/1000,state.authorized_until)+'.';
  }
  async function refresh(){
    if(busy||document.hidden)return;busy=true;
    try{state=await api('/api/operator/update',{cache:'no-store',signal:AbortSignal.timeout(10000)});reachable=true;render();}
    catch{reachable=false;render();if(dialog?.open){message.textContent='Infuser is restarting or cannot be reached. Checking again shortly.';}}
    finally{busy=false;}
  }
  open.onclick=()=>{requestId=crypto.randomUUID().replaceAll('-','');message.textContent='';confirm.hidden=false;open.hidden=true;refresh();};
  cancel.onclick=()=>{confirm.hidden=true;open.hidden=false;open.focus();};
  action.onclick=async()=>{
    if(action.disabled)return;submitting=true;render();
    try{
      await api('/api/operator/update',{method:'POST',body:JSON.stringify({request_id:requestId,current_build:state.current_build})});
      confirm.hidden=true;open.hidden=false;
      message.textContent='One update authorized. Checking for the latest release…';await refresh();
    }catch(error){message.textContent=error.message||'Could not authorize the update. Try again.';}
    finally{submitting=false;render();}
  };
  setInterval(()=>{render();if(dialog?.open)refresh();},5000);
  setInterval(refresh,30000);
  document.addEventListener('visibilitychange',()=>{if(!document.hidden)refresh();});
  refresh();
  return refresh;
}
