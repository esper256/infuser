// Run by platform_journey. Proxy the two real HTTPS origins to its disposable server.
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'../services/peerjs/node_modules/playwright/index.mjs');
const [base,library,game,ticket]=process.argv.slice(2);
const browser=await chromium.launch({channel:'chromium',executablePath:process.env.INFUSER_CHROMIUM});
try {
  const context=await browser.newContext({viewport:{width:360,height:640}});
  await context.addCookies([
    {name:'__Host-infuser_game',value:ticket,url:'https://'+game,secure:true,httpOnly:true},
    {name:'member',value:'user',url:'https://'+library,secure:true,httpOnly:true}
  ]);
  await context.route('**/*',async route=>{
    const req=route.request(),url=new URL(req.url());
    if(![library,game].includes(url.hostname))return route.abort();
    const response=await context.request.fetch(base+url.pathname+url.search,{
      method:req.method(),headers:{...await req.allHeaders(),host:url.hostname},data:req.postDataBuffer()??undefined
    });
    await route.fulfill({response});
  });
  const send=async text=>{
    const response=await context.request.post(base+'/api/chat',{headers:{
      host:library,origin:'https://'+library,cookie:'member=admin'
    },data:{client_id:crypto.randomUUID(),text}});
    assert.equal(response.status(),200);
  };
  const preview=r=>new URL(r.url()).pathname.startsWith('/api/game-chat/')&&r.status()===200;
  async function open(){
    const page=await context.newPage();
    const baseline=page.waitForResponse(preview);
    await page.goto('https://'+game+'/_infuser/play');
    await baseline;
    const mute=page.getByRole('button',{name:'Mute group chat popups'});
    assert.equal(await mute.getAttribute('aria-pressed'),'false');
    const log=page.frameLocator('#chat-overlay').getByRole('log');
    assert.equal(await log.innerText(),'');
    return {page,mute,log};
  }
  const {page,mute,log}=await open();
  assert(await page.evaluate(()=>crossOriginIsolated),'threaded games keep isolation');
  assert.equal(await page.locator('#chat-overlay').evaluate(f=>f.contentDocument),null,'game origin cannot read chat');
  const frame=page.locator('#chat-overlay');
  const clip=await frame.boundingBox();
  const idle=await page.screenshot({clip});
  await frame.evaluate(f=>{f.hidden=true;});
  assert(idle.equals(await page.screenshot({clip})),'idle overlay leaves game pixels untouched');
  await frame.evaluate(f=>{f.hidden=false;});
  await send('Hello <b>players</b>!');
  await log.getByText('Hello <b>players</b>!',{exact:true}).waitFor();
  assert.equal(await log.locator('strong').innerText(),'Admin');
  assert.equal(await page.locator('#game').evaluate(f=>document.activeElement===f),true);
  assert.equal(await page.locator('#chat-overlay').evaluate(f=>getComputedStyle(f).pointerEvents),'none');
  await log.locator('article').waitFor({state:'detached',timeout:10000});
  await page.locator('#fullscreen').click();
  await send('Fullscreen arrival');
  await log.getByText('Fullscreen arrival').waitFor();
  await page.evaluate(()=>document.exitFullscreen());
  await mute.click();
  assert.equal(await mute.getAttribute('aria-pressed'),'true');
  const answers=await frame.contentFrame().locator('body').evaluate(async()=>{
    const profile=new URLSearchParams(location.hash.slice(1)).get('profile');
    async function query(owner){
      const channel=new MessageChannel();
      const answer=new Promise(resolve=>{channel.port1.onmessage=e=>resolve(e.data);});
      navigator.serviceWorker.dispatchEvent(new MessageEvent('message',{
        data:{type:'infuser-reading-chat',owner},ports:[channel.port2]
      }));
      try{return await answer;}finally{channel.port1.close();channel.port2.close();}
    }
    return [await query(profile),await query('other-player')];
  });
  assert.deepEqual(answers,[true,false],'a muted game keeps only its own profile’s push quiet');
  await send('During mute');
  const second=await open();
  assert.equal(await mute.getAttribute('aria-pressed'),'true','two windows have independent mute controls');
  await second.page.close();
  const resume=page.waitForResponse(preview);
  await mute.click();
  await resume;
  await send('After mute');
  await log.getByText('After mute').waitFor();
  assert(!((await log.innerText()).includes('During mute')));
  console.log('Browser: silent group chat previews, fullscreen, expiry, per-window mute and origin isolation passed.');
} finally {await browser.close();}
