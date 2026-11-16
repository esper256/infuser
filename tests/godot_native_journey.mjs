// Opt-in native acceptance. Run godot_fixture with --domain games.localhost and
// --native-linux EXPORT first. Export tests/godot_native_probe.gd as an autoload
// only in the disposable test project, with the local test CA bundled.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import https from 'node:https';
import {createHash} from 'node:crypto';
import {spawn} from 'node:child_process';
const hash=s=>createHash('sha256').update(s).digest('hex');
const root=path.resolve('.local/native-acceptance');
const server='https://games.localhost:17878';
const ca=await fs.readFile('.local/godot/native-ca.pem');
const proxy=https.createServer({key:await fs.readFile('.local/godot/native-key.pem'),cert:ca},(req,res)=>{
  // The production router uses HTTPS's standard port. Translate only this
  // loopback fixture's headers/JSON so acceptance doesn't require root on 443.
  const headers={...req.headers,host:req.headers.host?.replace(':17878','')};
  if(headers.origin)headers.origin=headers.origin.replace(':17878','');
  const upstream=http.request({hostname:'127.0.0.1',port:17877,path:req.url,method:req.method,headers},r=>{
    if(r.headers['content-type']?.includes('application/json')){
      const chunks=[];r.on('data',c=>chunks.push(c));r.on('end',()=>{const body=Buffer.concat(chunks).toString().replace(/https:\/\/([a-z0-9.-]*localhost)(?!:)/g,'https://$1:17878').replace('server=https%3A%2F%2Fgames.localhost&','server=https%3A%2F%2Fgames.localhost%3A17878&');const headers={...r.headers};delete headers['content-length'];res.writeHead(r.statusCode,headers);res.end(body);});
    }else{res.writeHead(r.statusCode,r.headers);r.pipe(res);}
  });
  upstream.on('error',()=>{res.writeHead(502);res.end();});req.pipe(upstream);
});
await new Promise((resolve,reject)=>{proxy.once('error',reject);proxy.listen(17878,'127.0.0.1',resolve);});
async function api(route,body,member=false){
  return new Promise((resolve,reject)=>{
    const r=https.request(server+route,{ca,method:'POST',headers:{'Content-Type':'application/json',...(member?{cookie:'member=admin',Origin:server}:{})}},r=>{
      const chunks=[];r.on('data',c=>chunks.push(c));r.on('end',()=>{try{assert.equal(r.statusCode,200,Buffer.concat(chunks).toString());resolve(JSON.parse(Buffer.concat(chunks)));}catch(e){reject(e);}});
    });r.on('error',reject);r.end(JSON.stringify(body));
  });
}
try{
  await fs.mkdir(root,{recursive:true,mode:0o700});
  const verifier='c'.repeat(64),pair=await api('/api/native/v1/device/start',{challenge:hash(verifier)});
  await api('/api/native/v1/device/approve',{request_id:pair.request_id},true);
  const device=await api('/api/native/v1/device/token',{request_id:pair.request_id,verifier});
  await fs.mkdir(path.join(root,'connections'),{recursive:true,mode:0o700});
  await fs.writeFile(path.join(root,'connections',hash(server)+'.json'),JSON.stringify({server,token:device.token,name:'Test Player'}),{mode:0o600});
  async function play(guest,expected){
    const launch=await api('/api/native/v1/play/lantern-lake',{platform:'linux-x86_64',guest},true);
    const report=path.join(root,'report.json');await fs.rm(report,{force:true});
    await new Promise((resolve,reject)=>{
      const child=spawn('desktop/target/debug/examples/acceptance',[root,path.resolve('.local/godot/native-ca.pem'),launch.url],{env:{...process.env,XDG_DATA_HOME:path.join(root,'data'),INFUSER_ACCEPTANCE_REPORT:report},stdio:['ignore','pipe','pipe']});
      let output='';child.stdout.on('data',b=>{output+=b;});child.stderr.on('data',b=>{output+=b;});
      const timer=setTimeout(()=>{child.kill();reject(Error('Native launch timed out: '+output));},45000);
      child.on('error',reject);child.on('exit',code=>{clearTimeout(timer);if(code===0)resolve();else reject(Error('Native launch failed: '+output));});
    });
    const result=JSON.parse(await fs.readFile(report,'utf8'));
    assert.equal(result.guest,guest);assert.equal(result.before,expected);assert.equal(result.after,expected+3);assert.ok(result.revision);
    if(guest){assert.equal(result.previous_local,null);await assert.rejects(fs.stat(result.user_dir),{code:'ENOENT'});}
    return result;
  }
  const first=await play(false,0);assert.equal(first.previous_local,null);
  const second=await play(false,3);assert.equal(second.user_dir,first.user_dir);assert.equal(second.previous_local.best,3);
  const guest=await play(true,0);assert.notEqual(guest.user_dir,first.user_dir);
  const last=await play(false,6);assert.equal(last.previous_local.best,6);
  console.log('Native Godot: verified download, launch, cloud save/reload, persistent owner directory, blank guest and guest cleanup passed.');
}finally{await new Promise(resolve=>proxy.close(resolve));}
