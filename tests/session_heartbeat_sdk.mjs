// Run both shipped SDK entry points with a manual interval and controlled responses.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';

for(const path of ['../sdk/infuser.js','../sdk/v1/infuser.js']){
  test(path+' stops once on terminal status, including in-flight heartbeats', async()=>{
    const source=readFileSync(new URL(path,import.meta.url),'utf8')
      .replace(/^import .*;$/m,'const checkpoints = {};').replace(/^export /gm,'');
    for(const status of [401,403,404,409,410]){
      const events=[],messages=[],posts=[];
      let callback,clears=0;
      const window=new EventTarget();
      window.parent={postMessage:(data,origin)=>messages.push([data.status,origin])};
      window.addEventListener('infuser-session-error',event=>events.push(event.detail.status));
      const fetch=(url,options)=>{
        if(url.endsWith('/context'))return Promise.resolve(Response.json({}));
        assert.equal(options.method,'POST');
        return new Promise(resolve=>posts.push(resolve));
      };
      vm.runInNewContext(source,{window,fetch,CustomEvent,location:{origin:'https://game.invalid'},
        setInterval(fn){callback=fn;return 1;},clearInterval(id){assert.equal(id,1);clears++;}});
      await window.Infuser.ready;
      posts[0](new Response('gateway',{status:502}));
      await new Promise(setImmediate);
      assert.equal(clears,0);
      const pending=[callback(),callback()];
      posts[1](new Response('session expired',{status}));
      posts[2](Response.json({error:'session expired'},{status}));
      await Promise.all(pending);
      await callback(); // A callback already queued before clearInterval must also be harmless.
      assert.equal(posts.length,3);
      assert.equal(clears,1);
      assert.deepEqual(events,[502,status]);
      assert.deepEqual(messages,[[status,'https://game.invalid']]);
    }
  });
}
