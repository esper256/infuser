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
// Device-local settings shared with the service worker, independent of login cookies.
const cacheName='infuser-chat-alerts-v1',prefKey='/chat-alert/preference';

export async function clearNotifications(){
  if('serviceWorker' in navigator){
    const reg=await navigator.serviceWorker.getRegistration('/');
    const sub=await reg?.pushManager?.getSubscription();
    if(sub)await sub.unsubscribe();
    for(const notification of await reg?.getNotifications()||[])notification.close();
  }
  if('caches' in window)await caches.delete(cacheName).catch(()=>{});
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
  const sound=document.createElement('button');sound.type='button';sound.hidden=true;
  const test=document.createElement('button');test.type='button';test.hidden=true;test.textContent='Send test notification';
  panel.insertBefore(sound,hint);panel.insertBefore(test,hint);
  const supported=()=>registration&&'PushManager' in window&&'Notification' in window;
  let subscription,busy=false,deviceSound=false;
  const state=()=>{
    button.textContent=subscription?'Chat notifications: on':'Chat notifications: off';
    button.setAttribute('aria-pressed',String(!!subscription));
    sound.hidden=test.hidden=!subscription||!!me.inspecting;
    sound.textContent=deviceSound?'Sound: device default':'Sound: quiet';
    sound.setAttribute('aria-pressed',String(deviceSound));
    button.disabled=sound.disabled=test.disabled=busy;
  };
  async function saveSound(value){
    const cache=await caches.open(cacheName);
    await cache.put(prefKey,new Response(JSON.stringify({owner:me.player_id,sound:value})));
    deviceSound=value;
  }
  sound.onclick=async()=>{
    if(busy||!subscription)return;
    busy=true;state();
    try{await saveSound(!deviceSound);hint.textContent=deviceSound?'Uses your phone or computer’s notification settings.':'Quiet on this device.';}
    catch{hint.textContent='Could not save the sound setting. Try again.';}
    finally{busy=false;state();}
  };
  test.onclick=async()=>{
    if(busy||!subscription)return;
    busy=true;state();
    try{
      // A requested test starts a fresh burst. The push itself follows the normal delivery path.
      const cache=await caches.open(cacheName);
      const last='/chat-alert/last/'+encodeURIComponent(me.player_id),previous=await cache.match(last);
      await cache.delete(last);
      try{await request('/api/notifications/test',{method:'POST',body:JSON.stringify(clean(subscription))});}
      catch(error){
        // A rejected test shouldn't interrupt a real conversation's quiet period.
        if(previous&&!await cache.match(last))await cache.put(last,previous);
        throw error;
      }
      hint.textContent='Test sent. Check this device’s notifications.';
    }catch(error){hint.textContent=error.message||'Could not send the test. Try again.';}
    finally{busy=false;state();}
  };
  navigator.serviceWorker?.addEventListener('message',event=>{
    if(event.data?.type!=='infuser-reading-chat'||!event.ports[0])return;
    const panel=document.getElementById('chat-panel');
    event.ports[0].postMessage(event.data.owner===me.player_id&&!me.inspecting&&
      !document.hidden&&document.hasFocus()&&!!panel&&!panel.hidden&&!document.querySelector('dialog[open]'));
  });
  const ownerKey='infuser-push-owner';
  async function refresh(){
    if(!supported()||me.inspecting)return;
    const reg=await registration;subscription=await reg.pushManager.getSubscription();
    if(subscription && localStorage.getItem(ownerKey)!==me.player_id){await clearNotifications();subscription=null;}
    if(subscription){
      await request('/api/notifications',{method:'PUT',body:JSON.stringify(clean(subscription))});
      const cache=await caches.open(cacheName),pref=await (await cache.match(prefKey))?.json();
      deviceSound=pref?.owner===me.player_id&&pref.sound===true;
      if(!pref)hint.textContent='Already subscribed. Tap Sound to hear new chats on this device.';
    }
    state();
  }
  button.onclick=async()=>{
    if(busy)return;
    if(iphone()&&!standalone()){hint.textContent='Add Infuser to your Home Screen first, then enable notifications there.';return;}
    if(!supported()){hint.textContent='This browser does not support notifications. Try Safari, Chrome, Edge or Firefox.';return;}
    busy=true;state();hint.textContent='';
    try {
      if(subscription){
        await request('/api/notifications',{method:'DELETE',body:JSON.stringify({endpoint:subscription.endpoint})});
        await clearNotifications();subscription=null;deviceSound=false;
        hint.textContent='Off on this device.';
      }else{
        // Ask directly from the tap, before any network await (required on iOS).
        const permission=await Notification.requestPermission();
        if(permission!=='granted'){hint.textContent=permission==='denied'?'Notifications are blocked. Allow them in your browser or device settings to try again.':'Notifications stayed off.';return;}
        const config=await request('/api/notifications'),reg=await registration;
        const sub=await reg.pushManager.subscribe({userVisibleOnly:true,applicationServerKey:keyBytes(config.public_key)});
        try{await saveSound(true);await request('/api/notifications',{method:'PUT',body:JSON.stringify(clean(sub))});}
        catch(error){await sub.unsubscribe();throw error;}
        subscription=sub;localStorage.setItem(ownerKey,me.player_id);hint.textContent='On for this device. Your notification settings control the sound.';
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
      if('caches' in window)await caches.delete(cacheName).catch(()=>{});
      localStorage.removeItem(ownerKey);
      const notifications=await reg?.getNotifications()||[];for(const n of notifications)n.close();
      forgetProfile();
      location.assign(out.href);
    }catch{hint.textContent='Could not turn off notifications before signing out. Reconnect and try again.';}
  });
}
