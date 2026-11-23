// A visit is selected per tab. Game launches carry their own immutable context.
const KEY='infuser.visit';
export let selectedVisit=null;
try{selectedVisit=sessionStorage.getItem(KEY)||null;}catch{/* Home works without session storage. */}
export function visitHeaders(){return selectedVisit?{'X-Infuser-Visit':selectedVisit}:{};}
export function returnHome(){sessionStorage.removeItem(KEY);location.assign('/library');}
function enter(id){sessionStorage.removeItem('infuser.group-inspection');sessionStorage.setItem(KEY,id);location.assign('/library');}
const el=(tag,text)=>{const n=document.createElement(tag);if(text!==undefined)n.textContent=text;return n;};
let banner;
export function updateVisit(visit){
  if(!visit){banner?.remove();banner=null;return;}
  if(!banner){banner=el('aside');banner.id='visit-banner';banner.className='group-row';banner.setAttribute('aria-label','Visit');document.querySelector('body > header').after(banner);}
  const names=visit.visitors.map(v=>v.name).join(', '),left=Math.max(0,Math.ceil((visit.expires_at*1000-Date.now())/60000));
  const label=el('span',`${visit.name} · Visitors: ${names||'none'} · ${left<=5?`Ends in ${left} min`:'Ends '+new Date(visit.expires_at*1000).toLocaleTimeString([],{hour:'numeric',minute:'2-digit'})}`);
  banner.replaceChildren(label);
  if(selectedVisit){const home=el('button','Return home');home.onclick=returnHome;banner.append(home);}
}
export async function initVisits({me,api}){
  if(me.inspecting)return;
  const panel=document.querySelector('.account-menu'),button=el('button','Visits');button.id='visits-button';button.type='button';
  const group=panel.querySelector('#account-group');if(group)group.after(button);else panel.prepend(button);
  const dialog=el('dialog');dialog.id='visits-dialog';dialog.setAttribute('aria-label','Visits');
  const heading=el('div');heading.className='panel-heading';const close=el('button','×');close.className='close';close.setAttribute('aria-label','Close visits');close.onclick=()=>dialog.close();heading.append(el('h2','Visits'),close);
  const list=el('div'),error=el('p');error.setAttribute('role','status');dialog.append(heading,list,error);document.body.append(dialog);
  async function load(){
    list.replaceChildren();const data=await api(me.operator?'/api/operator/visits':'/api/visits');
    for(const v of data.visits.filter(v=>v.active)){
      const row=el('section');row.append(el('h3',v.name),el('p','Ends '+new Date(v.expires_at*1000).toLocaleString()));
      if(!me.operator || v.host_group===me.home_group || v.participants.some(p=>p.id===me.player_id&&!p.ended)){
        const join=el('button',v.id===selectedVisit?'Return home':'Visit');join.onclick=()=>v.id===selectedVisit?returnHome():enter(v.id);row.append(join);
      }
      if(me.operator){
        const end=el('button','End visit');end.onclick=async()=>{try{await api('/api/operator/visits/'+v.id,{method:'DELETE'});await load();}catch(e){error.textContent=e.message;}};
        const extend=el('button','Add an hour');extend.onclick=async()=>{try{await api('/api/operator/visits/'+v.id,{method:'PUT',body:JSON.stringify({expires_at:v.expires_at+3600})});await load();}catch(e){error.textContent=e.message;}};
        row.append(extend,end);
        for(const person of v.participants.filter(p=>!p.ended)){const remove=el('button','End visit for '+person.name);remove.onclick=async()=>{try{await api(`/api/operator/visits/${v.id}/participants/${person.id}`,{method:'DELETE'});await load();}catch(e){error.textContent=e.message;}};row.append(remove);}
      }
      list.append(row);
    }
    if(!me.operator){if(!list.childElementCount)list.append(el('p','No visits right now.'));return;}
    if(!data.enabled){list.append(el('p','Enable visits in the host configuration to begin.'));return;}
    const form=el('form');form.className='group-form';
    function field(label,input){const l=el('label',label);l.append(input);form.append(l);return input;}
    const name=field('Visit name',el('input'));name.required=true;name.maxLength=60;name.name='name';
    const host=field('Host group',el('select'));host.name='host_group';for(const [id,title] of Object.entries(data.groups)){const o=el('option',title);o.value=id;host.append(o);}host.value=me.home_group;
    const end=field('Ends',el('input'));end.type='datetime-local';end.required=true;end.name='expires_at';const date=new Date(Date.now()+4*3600000);date.setMinutes(date.getMinutes()-date.getTimezoneOffset());end.value=date.toISOString().slice(0,16);
    const people=el('fieldset'),games=el('fieldset');people.append(el('legend','Visitors'));games.append(el('legend','Games to bring'));form.append(people,games);
    function choice(parent,value,label){const l=el('label'),box=el('input');box.type='checkbox';box.value=value;l.append(box,document.createTextNode(label));parent.append(l);return box;}
    function gamesForPeople(){const selected=new Set([...games.querySelectorAll('input:checked')].map(i=>i.value));const groups=new Set([...people.querySelectorAll('input:checked')].map(i=>data.members.find(m=>m.id===i.value).group));games.replaceChildren(el('legend','Games to bring'));for(const g of data.games.filter(g=>groups.has(g.group)))choice(games,g.id,g.title+' · '+data.groups[g.group]).checked=selected.has(g.id);}
    function peopleForHost(){people.replaceChildren(el('legend','Visitors'));for(const m of data.members.filter(m=>m.group!==host.value)){choice(people,m.id,m.name+' · '+data.groups[m.group]).onchange=gamesForPeople;}gamesForPeople();}
    host.onchange=peopleForHost;peopleForHost();
    const create=el('button','Start visit');create.type='submit';form.append(create);
    form.onsubmit=async event=>{event.preventDefault();error.textContent='';create.disabled=true;try{
      await api('/api/operator/visits',{method:'POST',body:JSON.stringify({name:name.value,host_group:host.value,expires_at:Math.floor(new Date(end.value).getTime()/1000),participants:[...people.querySelectorAll('input:checked')].map(i=>i.value),games:[...games.querySelectorAll('input:checked')].map(i=>i.value)})});await load();
    }catch(e){error.textContent=e.message;}finally{create.disabled=false;}};
    list.append(form);
  }
  button.onclick=async()=>{button.closest('details').open=false;dialog.showModal();error.textContent='';try{await load();}catch(e){error.textContent=e.message;}};
}
