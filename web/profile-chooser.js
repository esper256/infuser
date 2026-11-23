import {nextPage,pinProfile,forgetProfile} from '/assets/profiles.js';
import {clearNotifications} from '/assets/pwa.js';
const list=document.getElementById('profile-list'),status=document.getElementById('profile-status');
const next=nextPage(new URLSearchParams(location.search).get('next')||'/library');
let busy=false;
const pushOwner=()=>{try{return localStorage.getItem('infuser-push-owner');}catch{return null;}};
async function choose(profile,multiple){
  if(busy)return;busy=true;
  const buttons=[...list.querySelectorAll('button')];buttons.forEach(b=>b.disabled=true);
  status.textContent='Opening '+profile.group.name+'…';
  try{
    // A push endpoint belongs to one profile. Never carry it into a different one.
    if(pushOwner()!==profile.player_id)await clearNotifications();
    window.InfuserThemes?.cacheProfile(profile.player_id,profile.appearance);
    forgetProfile();pinProfile(profile.player_id,multiple);
    location.replace(next);
  }catch(error){status.textContent=error.message||'Could not switch profiles. Try again.';busy=false;buttons.forEach(b=>b.disabled=false);}
}
document.getElementById('profile-signout').onclick=async event=>{
  event.preventDefault();const href=event.currentTarget.href;try{await clearNotifications();forgetProfile();location.assign(href);}
  catch{status.textContent='Could not turn off notifications. Reconnect and try again.';}
};
try{
  const response=await fetch('/api/profiles',{cache:'no-store'}),result=await response.json();
  if(!response.ok){
    if(response.status===401){const signin=document.getElementById('profile-signin');signin.hidden=false;signin.href='/oauth2/start?rd='+encodeURIComponent(location.pathname+location.search);}
    throw Error(result.error||'Could not load your profiles.');
  }
  const notification=new URLSearchParams(location.search).get('notification')==='chat'
    ?result.profiles.find(p=>p.player_id===pushOwner()):null;
  if(result.profiles.length===1)await choose(result.profiles[0],false);
  else if(notification)await choose(notification,true);
  else{
    status.textContent='';
    for(const profile of result.profiles){
      const button=document.createElement('button');button.type='button';button.className='profile-choice';button.dataset.profile=profile.player_id;
      if(profile.avatar){const image=document.createElement('img');image.src='/assets/avatars/'+encodeURIComponent(profile.avatar)+'/256.webp';image.alt='';image.width=112;image.height=112;button.append(image);}
      else{const avatar=document.createElement('span');avatar.className='profile-choice-fallback';avatar.textContent=profile.name.slice(0,2).toUpperCase();avatar.setAttribute('aria-hidden','true');button.append(avatar);}
      const name=document.createElement('strong'),group=document.createElement('span');name.textContent=profile.name;group.textContent=profile.group.name;
      button.append(name,group);button.onclick=()=>choose(profile,true);list.append(button);
    }
    list.querySelector('button')?.focus();
  }
}catch(error){status.textContent=error.message||'Could not reach Infuser. Refresh to try again.';}
