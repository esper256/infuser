// Real HTTPS, real service worker and notifications. Only the browser push-provider
// subscription is substituted: CI does not need a Google/Apple push registration.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import https from 'node:https';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'../services/peerjs/node_modules/playwright/index.mjs');
const [base,certPath,keyPath]=process.argv.slice(2);
const server=https.createServer({key:fs.readFileSync(keyPath),cert:fs.readFileSync(certPath)},(req,res)=>{
  const headers={...req.headers,host:'games.example.com'};
  if(headers.origin)headers.origin='https://games.example.com';
  const proxy=http.request(base+req.url,{method:req.method,headers},r=>{res.writeHead(r.statusCode,r.headers);r.pipe(res);});
  proxy.on('error',()=>{res.writeHead(502);res.end();});req.pipe(proxy);
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`https://localhost:${server.address().port}`;
const browser=await chromium.launch({channel:'chromium',executablePath:process.env.INFUSER_CHROMIUM,headless:true,args:['--ignore-certificate-errors']});
const errors=[];
try {
  const context=await browser.newContext({ignoreHTTPSErrors:true,permissions:['notifications'],viewport:{width:390,height:844}});
  await context.grantPermissions(['notifications'],{origin});
  await context.addCookies([{name:'member',value:'admin',url:origin}]);
  await context.addInitScript(()=>{
    window.permissionRequests=0;
    Notification.requestPermission=async()=>{window.permissionRequests++;return 'granted';};
    const fake=()=>({endpoint:localStorage.getItem('test-push'),unsubscribe:async()=>{localStorage.removeItem('test-push');return true;}});
    PushManager.prototype.getSubscription=async()=>localStorage.getItem('test-push')?fake():null;
    PushManager.prototype.subscribe=async function(options){
      if(!options.userVisibleOnly||options.applicationServerKey.byteLength!==65)throw new Error('Invalid push options');
      localStorage.setItem('test-push','https://fcm.googleapis.com/fcm/send/browser-fixture');return fake();
    };
  });
  context.on('page',p=>{
    p.on('pageerror',e=>errors.push(e.message));
    void p.addLocatorHandler(p.locator('#profile-dialog[open]'),()=>p.locator('#close-profile').click());
  });
  let page=await context.newPage();
  await page.goto(origin+'/library');await page.locator('#account-trigger').waitFor();
  await page.evaluate(()=>navigator.serviceWorker.ready);
  assert.equal(await page.evaluate(()=>window.permissionRequests),0,'no permission popup on load');
  assert.equal(await page.locator('link[rel=manifest]').getAttribute('href'),'/manifest.webmanifest');
  const cdp=await context.newCDPSession(page);
  assert.deepEqual((await cdp.send('Page.getAppManifest')).errors,[]);
  const manifest=await page.evaluate(()=>fetch('/manifest.webmanifest').then(r=>r.json()));assert.equal(manifest.name,'Infuser');
  await page.locator('#account-trigger').click();
  await page.getByRole('button',{name:'Install Infuser',exact:true}).click();
  assert.match(await page.locator('#notice').innerText(),/Install app|Home Screen/);
  const button=page.getByRole('button',{name:'Chat notifications: off',exact:true});await button.click();
  await page.getByRole('button',{name:'Chat notifications: on',exact:true}).waitFor();
  await page.getByRole('button',{name:'Sound: device default'}).waitFor();
  assert.equal(await page.evaluate(()=>window.permissionRequests),1);
  // Recreate an existing subscription from before the per-device preference existed.
  await page.evaluate(()=>caches.delete('infuser-chat-alerts-v1'));
  if(process.env.INFUSER_PWA_SCREENSHOT)await page.screenshot({path:process.env.INFUSER_PWA_SCREENSHOT});
  await page.reload();await page.locator('#account-trigger').click();await page.getByRole('button',{name:'Chat notifications: on',exact:true}).waitFor();
  assert.equal(await page.evaluate(()=>window.permissionRequests),0,'enabled devices do not ask again');
  await page.getByRole('button',{name:'Sound: quiet'}).waitFor();
  const worker=context.serviceWorkers()[0];assert(worker);
  await worker.evaluate(()=>{
    self.testNotifications=[];
    const show=self.registration.showNotification.bind(self.registration);
    self.registration.showNotification=async(title,options)=>{await show(title,options);self.testNotifications.push({title,...options});};
  });
  const push=()=>worker.evaluate(async()=>{let done;const event=new Event('push');event.waitUntil=p=>{done=p;};self.dispatchEvent(event);await done;});
  const lastNotice=()=>worker.evaluate(()=>self.testNotifications.at(-1));
  await push();assert.equal((await lastNotice()).silent,true,'legacy subscribers keep their quiet alerts');
  await page.getByRole('button',{name:'Sound: quiet'}).click();
  await page.getByRole('button',{name:'Sound: device default'}).waitFor();
  await page.reload();await page.locator('#account-trigger').click();
  await page.getByRole('button',{name:'Sound: device default'}).waitFor();
  let tested=0;
  await page.route('**/api/notifications/test',async route=>{
    assert.equal(route.request().method(),'POST');
    assert.equal(route.request().postDataJSON().endpoint,'https://fcm.googleapis.com/fcm/send/browser-fixture');
    tested++;await push();await route.fulfill({json:{ok:true}});
  });
  await page.getByRole('button',{name:'Send test notification'}).click();
  await page.getByText('Test sent. Check this device’s notifications.').waitFor();
  assert.equal(tested,1);assert.equal((await lastNotice()).renotify,true);
  await page.route('**/api/notifications/test',route=>route.fulfill({status:429,json:{error:'Wait 30 seconds before sending another test.'}}));
  await page.getByRole('button',{name:'Send test notification'}).click();
  await page.getByText('Wait 30 seconds before sending another test.').waitFor();
  await page.locator('#account-trigger').click();
  await page.evaluate(()=>{const b=document.createElement('button');b.id='test-fullscreen';b.textContent='Test fullscreen';b.onclick=()=>document.documentElement.requestFullscreen();document.body.append(b);});
  await page.locator('#test-fullscreen').click();await page.waitForFunction(()=>!!document.fullscreenElement);
  await context.grantPermissions(['notifications'],{origin});
  assert.equal(await page.evaluate(()=>Notification.permission),'granted');
  await push();
  assert.equal(await page.evaluate(()=>!!document.fullscreenElement),true,'arrival preserves fullscreen');
  const notice=await worker.evaluate(()=>self.testNotifications.at(-1));
  assert.equal(notice.title,'Infuser · Chat');assert.equal(notice.silent,true);assert.equal(notice.tag,'infuser-chat');
  // A synthetic click lacks OS user activation. Capture the worker's openWindow
  // destination and let a real new tab follow the notification/profile handoff.
  const destination=await worker.evaluate(async()=>{
    let url,done;self.clients.openWindow=async value=>{url=value;return null;};
    const event=new Event('notificationclick');event.notification={close(){}};event.waitUntil=p=>{done=p;};
    self.dispatchEvent(event);await done;return url;
  });
  await page.evaluate(()=>document.exitFullscreen());
  await page.close();
  await push();
  assert.equal((await lastNotice()).silent,true,'follow-ups stay quiet with the Library closed');
  // Advance the previous arrival, including across a worker lifetime with no open page.
  await worker.evaluate(async()=>{const c=await caches.open('infuser-chat-alerts-v1');const p=await(await c.match('/chat-alert/preference')).json();await c.put('/chat-alert/last/'+p.owner,new Response(String(Date.now()-300001)));});
  await Promise.all([push(),push()]);
  const pair=await worker.evaluate(()=>self.testNotifications.slice(-2));assert.equal(pair[0].silent,undefined);assert.equal(pair[0].renotify,true);assert.equal(pair[1].silent,true);
  page=await context.newPage();await page.goto(new URL(destination,origin).href);
  await page.locator('#chat-panel').waitFor();assert.equal(page.url(),origin+'/library');
  await page.bringToFront();await push();assert.equal((await lastNotice()).silent,true,'reading chat stays quiet');
  await page.reload();await page.locator('#chat-toggle').waitFor();assert(await page.locator('#chat-panel').isHidden());
  await push();assert.equal((await lastNotice()).renotify,true,'first message after leaving a chat you were reading can sound');
  // A link within the open app still works and preserves unrelated query parameters.
  await page.goto(origin+'/library?tab=games');await page.locator('#chat-toggle').waitFor();
  await page.evaluate(()=>{location.hash='chat';});await page.locator('#chat-panel').waitFor();
  assert.equal(page.url(),origin+'/library?tab=games');
  // An installed PWA can have an unconsumed notification URL from an earlier version.
  await page.evaluate(()=>history.replaceState(null,'','/library?tab=games&chat#chat'));
  await page.reload();await page.locator('#chat-toggle').waitFor();assert(await page.locator('#chat-panel').isHidden());
  assert.equal(page.url(),origin+'/library?tab=games');
  await page.evaluate(()=>sessionStorage.setItem('infuser.group-inspection',JSON.stringify({group:'away',expires:Date.now()+600000})));
  await page.goto(origin+'/library');await page.locator('#group-inspection').waitFor();
  await page.evaluate(()=>navigator.serviceWorker.dispatchEvent(new MessageEvent('message',{data:{type:'infuser-open-chat'}})));
  await page.locator('#chat-panel').waitFor({state:'visible'});await page.waitForURL('**/library');
  assert.equal(await page.locator('#group-inspection').count(),0,'home alert leaves operator inspection');
  assert.equal(await page.evaluate(()=>sessionStorage.getItem('infuser.group-inspection')),null);
  await page.locator('#chat-log').press('Escape');
  await page.locator('#account-trigger').click();await page.getByRole('button',{name:'Chat notifications: on',exact:true}).click();
  await page.getByRole('button',{name:'Chat notifications: off',exact:true}).waitFor();
  await page.getByRole('button',{name:'Chat notifications: off',exact:true}).click();
  await page.getByRole('button',{name:'Chat notifications: on',exact:true}).waitFor();
  await page.getByRole('link',{name:'Sign out',exact:true}).click();await page.waitForURL('**/oauth2/sign_out?rd=%2F');
  assert.equal(await page.evaluate(()=>localStorage.getItem('test-push')),null);
  assert.equal(await worker.evaluate(async()=>(await self.registration.getNotifications()).length),0);
  await page.goto(origin+'/library');await page.locator('#account-trigger').click();
  await page.evaluate(()=>{Notification.requestPermission=async()=>'denied';});
  await page.getByRole('button',{name:'Chat notifications: off',exact:true}).click();
  assert.match(await page.locator('.notification-hint').innerText(),/blocked/);
  assert.equal(await page.evaluate(()=>localStorage.getItem('test-push')),null);
  await page.evaluate(()=>Object.defineProperty(navigator,'userAgent',{value:'iPhone',configurable:true}));
  await page.getByRole('button',{name:'Chat notifications: off',exact:true}).click();
  assert.match(await page.locator('.notification-hint').innerText(),/Home Screen first/);
  await page.locator('#account-trigger').click();
  await context.setOffline(true);await page.goto(origin+'/library');
  assert.match(await page.locator('body').innerText(),/You're offline/);
  await context.setOffline(false);
  assert.deepEqual(errors,[]);
  console.log('PWA browser: legacy quiet, device sound, delivery test, burst cooldown, foreground chat, closed Library, fullscreen, open chat, off and sign-out passed.');
} finally {await browser.close();await new Promise(resolve=>server.close(resolve));}
