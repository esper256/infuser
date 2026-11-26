// Inspection belongs to one browser tab, never to the operator's group membership.
const KEY='infuser.group-inspection';
export let inspection=null;
try {
  const saved=JSON.parse(sessionStorage.getItem(KEY)||'null');
  if(saved&&typeof saved.group==='string'&&saved.expires>Date.now())inspection=saved;
  else sessionStorage.removeItem(KEY);
} catch { /* Home remains usable when session storage is unavailable. */ }
function home(){sessionStorage.removeItem(KEY);location.assign('/library');}
// Inspection starts at /library. Change the query too so this reloads the home
// membership instead of only changing the current tab's fragment.
export function openHomeChat(){sessionStorage.removeItem(KEY);sessionStorage.removeItem('infuser.visit');location.assign('/library?chat#chat');}
function inspect(group){sessionStorage.removeItem('infuser.visit');sessionStorage.setItem(KEY,JSON.stringify({group,expires:Date.now()+15*60*1000}));location.assign('/library');}
export function inspectionHeaders(){return inspection?{'X-Infuser-Inspect-Group':inspection.group}:{};}
export function initGroups({me,api,onRename}) {
  if(!me.operator)return;
  const el=(tag,text)=>{const n=document.createElement(tag);if(text)n.textContent=text;return n;};
  const button=el('button','Groups');button.id='operator-groups';button.title='Operator: private groups';button.type='button';
  // Operators retain Groups access while inspecting without admin controls.
  document.querySelector('#admin .admin-panel').prepend(button);
  button.setAttribute('aria-haspopup','dialog');
  const dialog=el('dialog');dialog.id='groups-dialog';dialog.setAttribute('aria-labelledby','groups-title');
  const heading=el('div');heading.className='panel-heading';const title=el('h2','Private groups');title.id='groups-title';
  const close=el('button','×');close.className='close';close.setAttribute('aria-label','Close groups');close.onclick=()=>dialog.close();heading.append(title,close);
  const list=el('div');list.className='group-list';const error=el('p');error.setAttribute('role','status');
  dialog.append(heading,list,error);
  const health=el('details'),healthBody=el('div');health.id='operator-health';health.append(el('summary','Host health'),healthBody);dialog.append(health);
  health.addEventListener('toggle',async()=>{
    if(!health.open)return;
    healthBody.replaceChildren(el('p','Checking…'));
    try {
      const status=await api('/api/operator/health');healthBody.replaceChildren();
      if(!status.configured){healthBody.append(el('p','Host monitoring is not configured.'));return;}
      healthBody.append(el('p',status.stale?'Monitoring data is stale. Check the host health timer.':'Checked '+new Date(status.report.at*1000).toLocaleString()));
      if(!status.report.backup_configured)healthBody.append(el('p','Backup monitoring is not configured.'));
      for(const check of status.report.checks){
        const row=el('p',(check.ok?'✓ ':'⚠ ')+check.detail);
        if(!check.ok)row.append(el('br'),document.createTextNode(check.action));
        healthBody.append(row);
      }
    }catch(problem){healthBody.replaceChildren(el('p',problem.message));}
  });
  async function load(){
    const result=await api('/api/operator/groups');list.replaceChildren();
    for(const group of result.groups){
      const row=el('div'),label=el('span',group.name),view=el('button',group.home?'Home':'View');
      row.className='group-row';
      view.type='button';view.onclick=()=>{try{if(group.home)home();else inspect(group.id);}catch{error.textContent='This browser needs session storage to open a temporary view.';}};
      row.append(label,view);
      if(!me.inspecting){
        const rename=el('button','Rename');
        rename.type='button';rename.setAttribute('aria-label','Rename '+group.name);rename.onclick=()=>editName(group,row,label,view,rename);
        row.insertBefore(rename,view);
      }
      list.append(row);
    }
  }
  function editName(group,row,label,view,rename){
    const form=el('form'),input=el('input'),save=el('button','Save'),cancel=el('button','Cancel');
    input.required=true;input.value=group.name;input.setAttribute('aria-label','New name for '+group.name);
    form.className='group-rename';cancel.type='button';
    function finish(){error.textContent='';row.replaceChildren(label,rename,view);rename.focus();}
    cancel.onclick=finish;
    input.onkeydown=event=>{if(event.key==='Escape'){event.preventDefault();finish();}};
    form.onsubmit=async event=>{
      event.preventDefault();if(save.disabled)return;
      save.disabled=cancel.disabled=input.disabled=true;error.textContent='';
      try{
        const updated=await api('/api/operator/groups/'+encodeURIComponent(group.id),{method:'PUT',body:JSON.stringify({name:input.value})});
        group.name=updated.name;label.textContent=updated.name;rename.setAttribute('aria-label','Rename '+updated.name);onRename(updated);finish();
      }catch(problem){error.textContent=problem.message;save.disabled=cancel.disabled=input.disabled=false;input.focus();}
    };
    form.append(input,save,cancel);row.replaceChildren(form);error.textContent='';input.focus();input.select();
  }
  if(!me.inspecting){
    const details=el('details');details.append(el('summary','Add a group'));
    const form=el('form');form.className='group-form';
    for(const [name,label,type,max] of [['name','Group name','text'],['admin_email',"Admin’s Google email",'email',253],['admin_name',"Admin’s display name",'text',40]]){
      const field=el('label',label),input=el('input');input.name=name;input.type=type;input.required=true;if(max)input.maxLength=max;field.append(input);form.append(field);}
    const create=el('button','Create group');form.append(create);details.append(form);dialog.append(details);
    form.onsubmit=async event=>{event.preventDefault();create.disabled=true;error.textContent='';
      try{await api('/api/operator/groups',{method:'POST',body:JSON.stringify(Object.fromEntries(new FormData(form)))});form.reset();details.open=false;await load();error.textContent='Created. Their admin can sign in with that Google account. No email is sent.';}
      catch(problem){error.textContent=problem.message;}finally{create.disabled=false;}};
  }
  document.body.append(dialog);button.onclick=async()=>{dialog.showModal();error.textContent='';try{await load();}catch(problem){error.textContent=problem.message;}};
  dialog.addEventListener('close',()=>button.focus({preventScroll:true}));
  if(me.inspecting){
    document.body.dataset.inspecting='true';
    const banner=el('aside');banner.id='group-inspection';banner.setAttribute('aria-label','Operator inspection');
    banner.append(el('span','Viewing '+me.group.name+' · Read only · You are not shown as online'));
    const leave=el('button','Return home');leave.type='button';leave.onclick=home;banner.append(leave);
    document.querySelector('body > header').after(banner);
    document.getElementById('add-game').hidden=true;
    document.getElementById('open-profile').disabled=true;
    const expiry=inspection?.expires||Date.now();setTimeout(home,Math.max(0,expiry-Date.now()));
  }
}
