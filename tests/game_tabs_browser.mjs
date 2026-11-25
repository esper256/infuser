// Uses the platform journey's authenticated HTTPS game origin and real save API.
import assert from 'node:assert/strict';

export async function gameTabsJourney(context, host, base) {
  const origin='https://'+host;
  const player=origin+'/_infuser/play';
  const notice='This game is also open in another tab.';
  const unsaved=' You have unsaved work here. Save it before switching tabs.';
  // Match the isolated export served by the platform fixture.
  await context.route(origin+'/', route=>route.fulfill({headers:{'Content-Type':'text/html','Cross-Origin-Embedder-Policy':'require-corp'},body:`<!doctype html>
    <title>Tab journey</title><button id="edit">Edit</button><script type="module">
    import {Infuser} from '/_infuser/sdk/v1.js';
    await Infuser.ready;
    document.querySelector('#edit').onclick=()=>Infuser.saves.setDirty(true);
    const report=()=>document.body.dataset.others=String(Infuser.tabs.others);
    addEventListener('infuser-tabs',report);report();
    </script>`}));
  async function open(url=player){
    const page=await context.newPage();
    await page.goto(url);
    const frame=page.frameLocator('#game');
    await frame.locator('body[data-others]').waitFor();
    return page;
  }
  async function close(page){
    await Promise.all([page.waitForEvent('close'),page.close({runBeforeUnload:true})]);
  }
  const count=(page,n)=>page.frameLocator('#game').locator('body[data-others="'+n+'"]').waitFor();
  const sdk=(page,fn)=>page.frameLocator('#game').locator('body').evaluate(fn);
  const banner=(page,text)=>page.getByText(text,{exact:true}).waitFor();
  const a=await open();
  const originalCookie=(await context.cookies(origin)).find(c=>c.name==='__Host-infuser_game').value;
  const library=await context.request.post(base+'/api/apps/moon/launch',{
    headers:{host:'games.example.com',origin:'https://games.example.com',cookie:'member=user'},data:{}
  });
  assert.ok(library.ok());
  const b=await open((await library.json()).url);
  const currentCookie=(await context.cookies(origin)).find(c=>c.name==='__Host-infuser_game').value;
  assert.ok(currentCookie!==originalCookie,'Library Play exchanged its own launch');
  const oldSession=await context.request.get(base+'/_infuser/api/v1/saves/two-tabs',{
    headers:{host,cookie:'__Host-infuser_game='+originalCookie}
  });
  assert.equal(oldSession.status(),200);
  await Promise.all([count(a,1),count(b,1),banner(a,notice),banner(b,notice)]);
  for(const page of [a,b]){
    assert.equal(await page.locator('#game').evaluate(el=>el.inert),false);
    const [strip,surface]=await Promise.all([page.locator('#other-tabs').boundingBox(),page.locator('#game').boundingBox()]);
    assert.ok(strip.y+strip.height<=surface.y);
  }
  await a.frameLocator('#game').locator('#edit').click();
  await banner(a,notice+unsaved);
  await banner(b,notice);
  await b.getByRole('button',{name:'Dismiss tab notice'}).click();
  assert.ok(await b.locator('#other-tabs').isHidden());
  // Both tabs can still save. A stale write cannot replace the winner's version.
  const version=await sdk(b,async()=> (await Infuser.saves.load('two-tabs')).version);
  await a.frameLocator('#game').locator('body').evaluate((_,v)=>Infuser.saves.save('two-tabs',{from:'a'},v),version);
  const conflict=await b.frameLocator('#game').locator('body').evaluate(async(_,v)=>{
    try { await Infuser.saves.save('two-tabs',{from:'b'},v); } catch(e) { return e.status; }
  },version);
  assert.equal(conflict,409);
  assert.deepEqual(await sdk(b,async()=> (await Infuser.saves.load('two-tabs')).data),{from:'a'});
  await sdk(b,()=>{
    Infuser.onBeforeLeave(reason=>localStorage.setItem('test-before-leave-'+reason,'called'));
  });
  await close(b);
  await count(a,0);
  await a.locator('#other-tabs').waitFor({state:'hidden'});
  assert.deepEqual(await sdk(a,()=>['hidden','pagehide'].map(reason=>localStorage.getItem('test-before-leave-'+reason))),['called','called']);

  const disappearing=await open();
  await Promise.all([count(a,1),count(disappearing,1),banner(a,notice+unsaved)]);
  await sdk(disappearing,()=>{
    const post=BroadcastChannel.prototype.postMessage;
    BroadcastChannel.prototype.postMessage=function(message){
      if(this.name==='infuser-tabs' && message.type==='bye')return;
      return post.call(this,message);
    };
  });
  await close(disappearing);
  const c=await open();
  await Promise.all([count(c,1),banner(c,notice)]);
  // A has received C's hello once C knows about A. Its roster contains C only.
  assert.equal(await sdk(a,()=>Infuser.tabs.others),1);
  await sdk(a,()=>Infuser.saves.setDirty(false));
  await banner(a,notice);
  const d=await open();
  await Promise.all([count(a,2),count(c,2),count(d,2)]);
  await close(d);
  await Promise.all([count(a,1),count(c,1)]);
  await close(c);
  await count(a,0);
  await a.close();
}
