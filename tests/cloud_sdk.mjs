// Run with Node's built-in runner; no npm dependencies or browser installation.
import baseTest from 'node:test';
import assert from 'node:assert/strict';
import { cloud as legacyCloud } from '../sdk/cloud-v1.js';
import { cloud as versionedCloud } from '../sdk/v1/cloud.js';
for(const [variant,cloud] of [['legacy',legacyCloud],['v1',versionedCloud]]) {
const test=(name,fn)=>baseTest(variant+': '+name,fn);

test('lost acknowledgement retries the identical checkpoint; conflicts and quota failures stay explicit', async()=>{
  const original=globalThis.fetch;
  try{
    const files={'progress.json':{schema:1,level:2}},bodies=[];
    globalThis.fetch=async(_url,options)=>{
      bodies.push(options.body);
      if(bodies.length===1){files['progress.json'].level=99;return new Response('{"status":', {status:200});}
      return Response.json({status:'saved',revision:'stored-on-first-attempt'});
    };
    const result=await cloud.commit('campaign',{commitId:'test-checkpoint-0001',expectedRevision:null,files});
    assert.equal(result.revision,'stored-on-first-attempt');assert.equal(bodies.length,2);assert.equal(bodies[0],bodies[1]);assert.equal(JSON.parse(bodies[1]).files['progress.json'].level,2);
    globalThis.fetch=async()=>Response.json({status:'conflict',current_revision:'cloud',candidate_revision:'device'},{status:409});
    assert.deepEqual(await cloud.commit('campaign',{commitId:'test-checkpoint-0002',expectedRevision:null,files}),{status:'conflict',current_revision:'cloud',candidate_revision:'device'});
    let calls=0;globalThis.fetch=async()=>{calls++;return Response.json({error:{code:'storage_full',message:'Keep the pending checkpoint'}},{status:507});};
    await assert.rejects(cloud.commit('campaign',{commitId:'test-checkpoint-0003',expectedRevision:null,files}),error=>error.code==='storage_full'&&error.status===507&&error.commitId==='test-checkpoint-0003');assert.equal(calls,1);
    globalThis.fetch=async()=>Response.json({});await assert.rejects(cloud.load('campaign'),error=>error.code==='invalid_response');
    await assert.rejects(cloud.commit('campaign',{commitId:'test-checkpoint-0004',files}),error=>error.code==='invalid_checkpoint');
  }finally{globalThis.fetch=original;}
});

}
