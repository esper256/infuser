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
export function openHomeChat(){sessionStorage.removeItem(KEY);location.assign('/library?chat#chat');}
function inspect(group){sessionStorage.setItem(KEY,JSON.stringify({group,expires:Date.now()+15*60*1000}));location.assign('/library');}
export function inspectionHeaders(){return inspection?{'X-Infuser-Inspect-Group':inspection.group}:{};}
export function initGroups({me,api}) {
  if(!me.operator)return;
  const el=(tag,text)=>{const n=document.createElement(tag);if(text)n.textContent=text;return n;};
  const button=el('button','Groups');button.id='operator-groups';button.title='Operator: private groups';button.type='button';
  document.getElementById('friends').after(button);
  const dialog=el('dialog');dialog.id='groups-dialog';dialog.setAttribute('aria-labelledby','groups-title');
  const heading=el('div');heading.className='panel-heading';const title=el('h2','Private groups');title.id='groups-title';
  const close=el('button','×');close.className='close';close.setAttribute('aria-label','Close groups');close.onclick=()=>dialog.close();heading.append(title,close);
  const list=el('div');list.className='group-list';const error=el('p');error.setAttribute('role','status');
  dialog.append(heading,list,error);
  async function load(){
    const result=await api('/api/operator/groups');list.replaceChildren();
    for(const group of result.groups){const row=el('div');row.className='group-row';const view=el('button',group.home?'Home':'View');
      view.type='button';view.onclick=()=>{try{if(group.home)home();else inspect(group.id);}catch{error.textContent='This browser needs session storage to open a temporary view.';}};
      row.append(el('span',group.name),view);list.append(row);}
  }
  if(!me.inspecting){
    const details=el('details');details.append(el('summary','Add a group'));
    const form=el('form');form.className='group-form';
    for(const [name,label,type,max] of [['name','Group name','text',60],['admin_email',"Adult’s Google email",'email',253],['admin_name',"Adult’s display name",'text',40]]){
      const field=el('label',label),input=el('input');input.name=name;input.type=type;input.required=true;input.maxLength=max;field.append(input);form.append(field);}
    const create=el('button','Create group');form.append(create);details.append(form);dialog.append(details);
    form.onsubmit=async event=>{event.preventDefault();create.disabled=true;error.textContent='';
      try{await api('/api/operator/groups',{method:'POST',body:JSON.stringify(Object.fromEntries(new FormData(form)))});form.reset();details.open=false;await load();error.textContent='Created. Their adult can sign in with that Google account. No email is sent.';}
      catch(problem){error.textContent=problem.message;}finally{create.disabled=false;}};
  }
  document.body.append(dialog);button.onclick=async()=>{dialog.showModal();error.textContent='';try{await load();}catch(problem){error.textContent=problem.message;}};
  dialog.addEventListener('close',()=>button.focus());
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
