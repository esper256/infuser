import {forgetProfile} from '/assets/profiles.js';
let installPrompt, registration;
const standalone=()=>matchMedia('(display-mode: standalone)').matches||navigator.standalone===true;
const iphone=()=>/iPad|iPhone|iPod/.test(navigator.userAgent)||(navigator.platform==='MacIntel'&&navigator.maxTouchPoints>1);
window.addEventListener('beforeinstallprompt',event=>{event.preventDefault();installPrompt=event;window.dispatchEvent(new Event('infuser-install-ready'));});
if('serviceWorker' in navigator){
  registration=navigator.serviceWorker.register('/sw.js',{scope:'/',updateViaCache:'none'}).then(()=>navigator.serviceWorker.ready);
  registration.catch(()=>{});
}
const keyBytes=value=>Uint8Array.from(atob(value.replaceAll('-','+').replaceAll('_','/')),c=>c.charCodeAt(0));
const clean=sub=>({endpoint:sub.endpoint});

export async function clearNotifications(){
  if('serviceWorker' in navigator){
    const reg=await navigator.serviceWorker.getRegistration('/');
    const sub=await reg?.pushManager?.getSubscription();
    if(sub)await sub.unsubscribe();
    for(const notification of await reg?.getNotifications()||[])notification.close();
  }
  try{localStorage.removeItem('infuser-push-owner');}catch{/* No stored owner. */}
}

export function setupPwa({panel,me,api,notice}) {
  const request=(path,options={})=>api(path,{...options,signal:AbortSignal.timeout(12000)});
  const install=document.createElement('button');install.type='button';install.textContent='Install Infuser';
  function installState(){install.hidden=standalone();}
  installState();window.addEventListener('appinstalled',installState);
  install.onclick=async()=>{
    if(installPrompt){const prompt=installPrompt;installPrompt=null;await prompt.prompt();await prompt.userChoice;installState();}
    else notice(iphone()?'Tap Share → Add to Home Screen → Add. Then open Infuser from your Home Screen.':'Use your browser menu → Install app or Add to Home Screen.');
  };
  panel.append(install);
  const button=document.createElement('button');button.type='button';button.textContent='Chat notifications';
  const hint=document.createElement('small');hint.className='notification-hint';hint.setAttribute('role','status');
  button.hidden=!!me.inspecting;panel.append(button,hint);
  const supported=()=>registration&&'PushManager' in window&&'Notification' in window;
  let subscription,busy=false;
  const state=()=>{button.textContent=subscription?'Chat notifications: on':'Chat notifications: off';button.setAttribute('aria-pressed',String(!!subscription));};
  const ownerKey='infuser-push-owner';
  async function refresh(){
    if(!supported()||me.inspecting)return;
    const reg=await registration;subscription=await reg.pushManager.getSubscription();
    if(subscription && localStorage.getItem(ownerKey)!==me.player_id){await clearNotifications();subscription=null;}
    if(subscription){await request('/api/notifications',{method:'PUT',body:JSON.stringify(clean(subscription))});}
    state();
  }
  button.onclick=async()=>{
    if(busy)return;
    if(iphone()&&!standalone()){hint.textContent='Add Infuser to your Home Screen first, then enable notifications there.';return;}
    if(!supported()){hint.textContent='This browser does not support notifications. Try Safari, Chrome, Edge or Firefox.';return;}
    busy=true;button.disabled=true;hint.textContent='';
    try {
      if(subscription){
        await request('/api/notifications',{method:'DELETE',body:JSON.stringify({endpoint:subscription.endpoint})});
        await subscription.unsubscribe();subscription=null;localStorage.removeItem(ownerKey);
        hint.textContent='Off on this device.';
      }else{
        // Ask directly from the tap, before any network await (required on iOS).
        const permission=await Notification.requestPermission();
        if(permission!=='granted'){hint.textContent=permission==='denied'?'Notifications are blocked. Allow them in your browser or device settings to try again.':'Notifications stayed off.';return;}
        const config=await request('/api/notifications'),reg=await registration;
        const sub=await reg.pushManager.subscribe({userVisibleOnly:true,applicationServerKey:keyBytes(config.public_key)});
        try{await request('/api/notifications',{method:'PUT',body:JSON.stringify(clean(sub))});}
        catch(error){await sub.unsubscribe();throw error;}
        subscription=sub;localStorage.setItem(ownerKey,me.player_id);hint.textContent='On for this device, even when Infuser is closed.';
      }
    }catch(error){hint.textContent=error.message||'Could not change notifications. Try again.';}
    finally{busy=false;button.disabled=false;state();}
  };
  button.disabled=true;
  refresh().catch(()=>{hint.textContent='Could not check notifications. Try again.';}).finally(()=>{button.disabled=false;});
  // Unsubscribe before signing out on a shared device. If it fails, keep the
  // session available for a retry instead of leaving a hidden subscription.
  const out=panel.querySelector('a[href*="sign_out"]');
  out?.addEventListener('click',async event=>{
    if(!supported()){forgetProfile();return;}
    event.preventDefault();
    try{
      const reg=await navigator.serviceWorker.getRegistration('/');
      const sub=await reg?.pushManager.getSubscription();
      if(sub){
        await sub.unsubscribe();
        // Revocation at the browser also works after membership/session expiry.
        // Inspection must not issue writes with another group's view header.
        if(!me.inspecting)await request('/api/notifications',{method:'DELETE',body:JSON.stringify({endpoint:sub.endpoint})}).catch(()=>{});
      }
      localStorage.removeItem(ownerKey);
      const notifications=await reg?.getNotifications()||[];for(const n of notifications)n.close();
      forgetProfile();
      location.assign(out.href);
    }catch{hint.textContent='Could not turn off notifications before signing out. Reconnect and try again.';}
  });
}
