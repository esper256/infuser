// Keep authenticated pages, chat, API responses and game assets on the network.
// A worker update never reloads a running game or changes fullscreen state.
self.addEventListener('install',event=>event.waitUntil(self.skipWaiting()));
self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));
self.addEventListener('fetch',event=>{
  if(event.request.mode!=='navigate'||new URL(event.request.url).origin!==self.location.origin)return;
  event.respondWith(fetch(event.request).catch(()=>new Response(`<!doctype html><html lang="en"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Infuser · Offline</title><body style="background:#0c1017;color:#fff;font:20px system-ui;padding:10vh 8vw"><h1>You're offline</h1><p>Reconnect to open your Library.</p><a style="color:#9be9ce" href="/library">Try again</a></body></html>`,{status:503,headers:{'Content-Type':'text/html','Cache-Control':'no-store'}})));
});
// Persist the last arrival so dismissing an alert or restarting the worker
// does not reset a burst. The page can reset it for an explicit delivery test.
const burstMs=5*60*1000;
async function readingChat(client,owner){
  const channel=new MessageChannel();
  try{
    return await new Promise(resolve=>{
      const timeout=setTimeout(()=>resolve(false),400);
      channel.port1.onmessage=event=>{clearTimeout(timeout);resolve(event.data===true);};
      client.postMessage({type:'infuser-reading-chat',owner},[channel.port2]);
    });
  }finally{channel.port1.close();}
}
async function chatNotification(){
  let audible=false;
  try{
    const cache=await caches.open('infuser-chat-alerts-v1');
    const preference=await (await cache.match('/chat-alert/preference'))?.json();
    if(preference?.sound&&preference.owner){
      const key='/chat-alert/last/'+encodeURIComponent(preference.owner);
      const windows=await self.clients.matchAll({type:'window'});
      // Keyboard focus belongs to the game, outside its Library-origin preview frame.
      const focused=windows.filter(client=>client.visibilityState==='visible'&&(client.focused||new URL(client.url).pathname==='/game-chat'));
      const reading=(await Promise.all(focused.map(client=>readingChat(client,preference.owner)))).some(Boolean);
      if(reading){
        await cache.delete(key);
      }else{
        const previous=Number(await (await cache.match(key))?.text()||0),now=Date.now();
        await cache.put(key,new Response(String(now)));
        audible=!previous||now-previous>=burstMs||now<previous;
      }
    }
  }catch{/* Missing settings or inaccessible storage falls back to a quiet alert. */}
  await self.registration.showNotification('Group Chat',{
    body:'Open your group chat.',icon:'/assets/app-192.png',tag:'infuser-chat',
    ...(audible?{renotify:true}:{silent:true,renotify:false}),data:{url:'/library#chat'}
  });
}
let pendingPush=Promise.resolve();
self.addEventListener('push',event=>{
  // Two push events can wake this worker before either finishes reading storage.
  const showing=pendingPush.then(chatNotification);
  pendingPush=showing.catch(()=>{});
  event.waitUntil(showing);
});
self.addEventListener('notificationclick',event=>{
  event.notification.close();
  event.waitUntil((async()=>{
    // Resolve the subscription's profile in a fresh page. Never reuse an
    // arbitrary tab that may be showing another group's conversation.
    await self.clients.openWindow('/profiles?notification=chat&next='+encodeURIComponent('/library?chat#chat'));
  })());
});
