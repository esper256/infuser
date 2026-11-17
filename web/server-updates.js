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
  const title=node('h3','Server updates');title.id='server-update-title';
  section.setAttribute('aria-labelledby',title.id);
  const header=node('div'),badge=node('span');header.className='update-heading';badge.className='update-state';header.append(title,badge);
  const builds=node('div');builds.className='update-builds';
  function buildCard(label){
    const card=node('div'),heading=node('h4',label),id=node('code'),age=node('p'),date=node('time');
    card.className='update-build';age.className='update-build-age';card.append(heading,id,age,date);builds.append(card);
    return {card,id,age,date};
  }
  const current=buildCard('Running now'),next=buildCard('Next update');next.card.classList.add('update-candidate');
  const publishing=node('div'),publishingTitle=node('strong','Game publishing is in progress'),publishingDetail=node('p','Restarting now can interrupt uploads, publishing, or server builds. Let this finish before updating.');
  publishing.className='update-publishing';publishing.setAttribute('role','status');publishing.append(publishingTitle,publishingDetail);publishing.hidden=true;
  const status=node('div'),reason=node('p'),blockers=node('ul'),wait=node('p'),checked=node('p'),message=node('p');
  status.className='update-status';reason.className='update-progress';wait.className=checked.className='update-caption';message.className='update-message';message.setAttribute('role','status');
  status.append(reason,blockers,wait,checked);
  const warning=node('p','Updating now can disconnect games and interrupt uploads or publishing in every private group. Unfinished work may need to be retried.');
  const action=node('button','Authorize one update'),cancel=node('button','Cancel'),open=node('button','Update anyway');action.type=cancel.type=open.type='button';
  const confirm=node('div');confirm.className='update-confirm';confirm.hidden=true;
  const actions=node('div');actions.className='dialog-actions';actions.append(cancel,action);
  const note=node('p','The updater rechecks the latest release before restarting, so a newer build may replace the one shown.');note.className='update-caption';
  confirm.append(warning,actions);section.append(header,builds,publishing,status,open,confirm,note,message);
  let state,busy=false,submitting=false,requestId,reachable=true;
  const set=(object,key,value)=>{if(object[key]!==value)object[key]=value;};
  const text=(element,value)=>set(element,'textContent',value);
  function showBuild(view,id,timestamp,empty){
    text(view.id,id?id.slice(0,12):empty);set(view.id,'title',id||'');
    const known=Number.isFinite(timestamp)&&timestamp>0;
    text(view.age,id?(known?'Built '+humanWait(timestamp)+' ago':'Build time unknown'):'');
    text(view.date,known?new Date(timestamp*1000).toLocaleString(undefined,{dateStyle:'medium',timeStyle:'short'}):'');
    if(known)set(view.date,'dateTime',new Date(timestamp*1000).toISOString());else if(view.date.hasAttribute('datetime'))view.date.removeAttribute('datetime');
  }
  function render(){
    set(section,'hidden',!state);
    if(!state)return;
    const phase=state.report?.phase;
    const phases={checking:'Checking for the latest release',downloading:'Downloading the latest release',activating:'Restarting into the update',failed:'The updater could not verify or install this release. Waiting for the next check.'};
    const stale=state.stale||!state.checked_at||Date.now()/1000-state.checked_at>=180;
    const authorized=state.authorized_until>Date.now()/1000;
    const isPublishing=state.publishing||(state.reasons||[]).some(r=>/publishing|uploads|server builds/i.test(r));
    set(publishing,'hidden',!isPublishing);
    // Publishing is independent of updater phase, reachability and player activity.
    // Never replace this warning with a less urgent waiting reason.
    text(badge,!reachable?'Connection lost':!state.configured?'Not configured':stale?'Check overdue':phase==='failed'?'Update failed':phase==='checking'?'Checking…':phase==='downloading'?'Downloading':phase==='activating'?'Restarting':state.available?'Update available':phase==='current'?'Up to date':'Awaiting check');
    set(badge.dataset,'attention',String(!reachable||stale||phase==='failed'));
    showBuild(current,state.current_build,state.current_built_at,'Unknown build');
    showBuild(next,state.available?state.report?.release:null,state.available?state.release_built_at:null,state.available?'Checking…':state.configured&&!stale&&phase==='current'?'Already on latest':'Not checked yet');
    const why=!reachable?'Waiting for Infuser to respond':!state.configured?'Automatic updates are not reporting to this server.':stale?'Waiting for a fresh update check':phases[phase]||(state.available?(authorized?'One update is authorized. Waiting for the updater.':'The update will install when activity is idle.'):'No update is waiting.');
    text(reason,why);
    const reasons=(state.reasons||[]).filter(r=>!/publishing|uploads|server builds/i.test(r)&&r!==why);
    const signature=JSON.stringify(reasons);
    if(blockers.dataset.reasons!==signature){blockers.replaceChildren(...reasons.map(r=>node('li',r)));blockers.dataset.reasons=signature;}
    set(blockers,'hidden',!reasons.length);
    text(wait,state.available?'Update waiting '+(state.available_since?humanWait(state.available_since):'less than a minute'):'');
    text(checked,state.checked_at?'Last checked '+humanWait(state.checked_at)+' ago':'');
    set(action,'disabled',submitting||!reachable||!state.available||stale||authorized||phase==='failed'||phase==='activating');
    set(open,'disabled',action.disabled);set(open,'hidden',!state.available||!confirm.hidden);
    if(!state.available)set(confirm,'hidden',true);
    set(note,'hidden',!state.available);
    text(open,authorized?'Update authorized':'Update anyway');
    if(authorized)text(message,'Authorized once. The updater will recheck the latest release on its next pass, usually within a minute. Authorization expires in '+humanWait(Date.now()/1000,state.authorized_until)+'.');
    else if(message.dataset.authorized==='true')text(message,'');
    set(message.dataset,'authorized',String(authorized));
  }
  async function refresh(){
    if(busy||document.hidden)return;busy=true;
    try{state=await api('/api/operator/update',{cache:'no-store',signal:AbortSignal.timeout(10000)});if(!reachable)message.textContent='';reachable=true;render();}
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
