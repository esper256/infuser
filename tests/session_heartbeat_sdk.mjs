// Run both shipped SDK entry points with a manual interval and controlled responses.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';

function load(path){
  return readFileSync(new URL(path,import.meta.url),'utf8')
      .replace(/^import \{sessionDenials.*;$/m,()=>readFileSync(new URL('../sdk/v1/session-fetch.js',import.meta.url),'utf8'))
      .replace(/^import \{gamepads\}.*;$/m,'const gamepads = {};').replace('export {gamepads};','')
      .replace(/^import .*;$/m,'const checkpoints = {};').replace(/^export /gm,'');
}

for(const path of ['../sdk/infuser.js','../sdk/v1/infuser.js']){
  test(path+' stops once on a marked denial, including in-flight heartbeats', async()=>{
    const source=load(path);
    for(const status of [401,403,404,409,410]){
      const events=[],messages=[],posts=[],updates=[],clientUpdates=[];
      let callback,clears=0;
      const window=new EventTarget();
      window.parent={postMessage:(data,origin)=>{
        if(data.type==='infuser-update-available')updates.push(origin);
        else if(data.type==='infuser-client-update')clientUpdates.push([data.version,data.force_reload,origin]);
        else messages.push([data.status,data.reason,data.message,origin]);
      }};
      window.addEventListener('infuser-session-error',event=>events.push([event.detail.status,event.detail.reason,event.detail.message]));
      const fetch=(url,options)=>{
        if(url.endsWith('/context'))return Promise.resolve(Response.json({}));
        assert.equal(options.method,'POST');
        return new Promise(resolve=>posts.push(resolve));
      };
      vm.runInNewContext(source,{window,fetch,EventTarget,CustomEvent,location:{origin:'https://game.invalid'},
        setInterval(fn){callback=fn;return 1;},clearInterval(id){assert.equal(id,1);clears++;}});
      await window.Infuser.ready;
      posts[0](new Response('gateway',{status:502}));
      await new Promise(setImmediate);
      assert.equal(clears,0);
      for(const [update_available,force_reload] of [[false,0],[true,0],[true,1]]){
        const beat=callback();
        posts.at(-1)(Response.json({update_available,client_update:{version:'v2',force_reload}}));
        await beat;
      }
      assert.deepEqual(updates,['https://game.invalid']);
      assert.deepEqual(clientUpdates,[['v2',0,'https://game.invalid'],['v2',1,'https://game.invalid']]);
      const pending=[callback(),callback()];
      const error={code:'session_ended',reason:'signed_out',message:'You signed out of the Library in this browser.'};
      posts[4](Response.json({error},{status,headers:{'Infuser-Session':'ended'}}));
      posts[5](Response.json({error:'session expired'},{status}));
      await Promise.all(pending);
      await callback(); // A callback already queued before clearInterval must also be harmless.
      assert.equal(posts.length,6);
      assert.equal(clears,1);
      assert.deepEqual(events,[[status,error.reason,error.message]]);
      assert.deepEqual(messages,[[status,error.reason,error.message,'https://game.invalid']]);
    }
  });
}

test('v1 wakes immediately and reports an authorization failure from a save just once',async()=>{
  const document=new EventTarget(),window=new EventTarget(),statuses=[],posts=[];
  window.parent={postMessage:m=>posts.push(m.status)};
  window.addEventListener('infuser-session-error',e=>statuses.push(e.detail.status));
  let beats=0,clears=0,saveStatus=409;
  const fetch=async(path)=>{
    if(/\/(peers|connections)\//.test(path))return Response.json({error:'ended'},{status:410});
    if(path.endsWith('/heartbeat'))beats++;
    return Response.json({}, {status:path.includes('/saves/')?saveStatus:200,headers:saveStatus===403?{'Infuser-Session':'ended'}:{}});
  };
  const context={document,window,fetch,EventTarget,CustomEvent,location:{origin:'https://game.invalid'},
    setInterval:()=>1,clearInterval:()=>clears++};
  vm.runInNewContext(load('../sdk/v1/infuser.js')+';this.platformFetch=sessionFetch;',context);
  const sdk=window.Infuser;
  await sdk.ready;
  assert.equal(beats,1);
  for(const state of ['hidden','visible']){
    document.visibilityState=state;document.dispatchEvent(new Event('visibilitychange'));
  }
  assert.equal(beats,2);
  for(const path of ['peers/room','connections/v1/attempt']){
    const response=await context.platformFetch('/_infuser/api/v1/'+path);
    assert.equal(response.status,410);
  }
  assert.deepEqual(statuses,[]);assert.equal(clears,0);
  await assert.rejects(sdk.saves.save('progress',{},0),e=>e.status===409);
  assert.deepEqual(posts,[]);
  saveStatus=403;
  await Promise.all([1,2].map(()=>assert.rejects(sdk.saves.load('progress'),e=>e.status===403)));
  assert.deepEqual(statuses,[403]);assert.deepEqual(posts,[403]);assert.equal(clears,1);
  document.dispatchEvent(new Event('visibilitychange'));assert.equal(beats,2);
});
