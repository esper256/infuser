// Keep authenticated pages, chat, API responses and game assets on the network.
// A worker update never reloads a running game or changes fullscreen state.
self.addEventListener('install',event=>event.waitUntil(self.skipWaiting()));
self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));
self.addEventListener('fetch',event=>{
  if(event.request.mode!=='navigate'||new URL(event.request.url).origin!==self.location.origin)return;
  event.respondWith(fetch(event.request).catch(()=>new Response(`<!doctype html><html lang="en"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Infuser · Offline</title><body style="background:#0c1017;color:#fff;font:20px system-ui;padding:10vh 8vw"><h1>You're offline</h1><p>Reconnect to open your Library.</p><a style="color:#9be9ce" href="/library">Try again</a></body></html>`,{status:503,headers:{'Content-Type':'text/html','Cache-Control':'no-store'}})));
});
self.addEventListener('push',event=>{
  event.waitUntil((async()=>{
    // No message text or private-group identity is sent to a lock screen.
    await self.registration.showNotification('Infuser · Chat',{
      body:'Your group has a new message.',icon:'/assets/app-192.png',
      tag:'infuser-chat',renotify:false,silent:true,data:{url:'/library#chat'}
    });
  })());
});
self.addEventListener('notificationclick',event=>{
  event.notification.close();
  event.waitUntil((async()=>{
    const tabs=await self.clients.matchAll({type:'window',includeUncontrolled:true});
    const tab=tabs.find(t=>{const u=new URL(t.url);return u.origin===self.location.origin&&['/','/library','/clubhouse'].includes(u.pathname);});
    if(tab){await tab.focus();tab.postMessage({type:'infuser-open-chat'});}
    else await self.clients.openWindow('/library#chat');
  })());
});
