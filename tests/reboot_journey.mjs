// Real shell/curl/flock against a local admission API; only time and systemd are fake.
// Requires Node, Bash, curl, jq and flock. Never invokes the host's systemctl.
import assert from 'node:assert/strict';
import http from 'node:http';
import {mkdtemp, writeFile, readFile, rm, chmod} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {spawn} from 'node:child_process';
import {once} from 'node:events';

const work=await mkdtemp(join(tmpdir(),'infuser-reboot-'));
const token='test_reboot_credential_01234567890123456789';
let status=200, posts=0, deletes=0, late=false;
const server=http.createServer(async(req,res)=>{
  assert.equal(req.url,'/v1/maintenance');
  assert.equal(req.headers.authorization,`Bearer ${token}`);
  let text='';for await(const part of req)text+=part;
  const body=JSON.parse(text);
  if(req.method==='POST'){
    posts++;assert.deepEqual(body,{purpose:'reboot'});
    if(late)await writeFile(join(work,'clock'),'0430');
    res.writeHead(status,{'content-type':'application/json'});
    const error=status===409?'A guest has "Pond" open in the browser.':'not admitted';
    res.end(JSON.stringify(status===200?{lease:{id:'fixture-lease'}}:{error}));
  }else{
    deletes++;assert.deepEqual(body,{id:'fixture-lease'});res.end('{}');
  }
});
server.listen(0,'127.0.0.1');await once(server,'listening');
const env={...process.env,PATH:`${work}:${process.env.PATH}`,FIXTURE_DIR:work,
  INFUSER_API_PORT:String(server.address().port),INFUSER_CONTROL_TOKEN_FILE:join(work,'token'),
  INFUSER_REBOOT_FLAG:join(work,'required'),INFUSER_HOST_LOCK:join(work,'lock'),
  INFUSER_MAINTENANCE_TIMEZONE:'America/Los_Angeles'};
const script=resolve('deploy/reboot-if-required.sh'),lock=resolve('deploy/host-maintenance-lock.sh');
async function command(file,args,overrides={}){
  const child=spawn(file,args,{env:{...env,...overrides}});
  let output='';child.stdout.on('data',b=>output+=b);child.stderr.on('data',b=>output+=b);
  const [code]=await once(child,'close');return {code,output};
}
async function run(args=[]){return command('bash',[lock,'bash',script,...args]);}
async function set(name,value){await writeFile(join(work,name),value);}
async function noReboot(){assert.equal(await readFile(join(work,'reboots'),'utf8'),'');}
try{
  await set('token',token);await set('clock','0400');await set('apt','inactive');await set('reboots','');
  await set('date','#!/bin/sh\ncat "$FIXTURE_DIR/clock"\n');
  await set('systemctl',`#!/bin/sh
if [ "$1" = show ]; then cat "$FIXTURE_DIR/apt"; exit; fi
if [ "$1" = --check-inhibitors=yes ] && [ "$2" = reboot ]; then
  test ! -e "$FIXTURE_DIR/inhibited" || exit 1
  echo reboot >> "$FIXTURE_DIR/reboots"
  exit 0
fi
exit 99
`);
  await chmod(join(work,'date'),0o755);await chmod(join(work,'systemctl'),0o755);
  assert.equal((await run()).code,0);assert.equal(posts,0);await noReboot();
  await set('required','');await set('clock','1200');
  assert.match((await run()).output,/outside/);assert.equal(posts,0);
  await set('clock','0400');await set('apt','active');
  assert.match((await run()).output,/package maintenance/);assert.equal(posts,0);
  await set('apt','inactive');status=409;
  const deferred=await run();
  assert.match(deferred.output,/Pond/);
  assert.match(deferred.output,/open in the browser/);
  assert.equal(posts,1);await noReboot();
  status=503;assert.notEqual((await run()).code,0);await noReboot();
  status=200;assert.equal((await run(['--check'])).code,0);assert.equal(deletes,1);await noReboot();
  late=true;assert.match((await run()).output,/window has ended/);assert.equal(deletes,2);await noReboot();
  late=false;await set('clock','0400');await set('inhibited','');
  assert.notEqual((await run()).code,0);assert.equal(deletes,3);await noReboot();
  await rm(join(work,'inhibited'));
  // A full backup/upload or deployment holding the host lock prevents even admission.
  const holder=spawn('flock',[env.INFUSER_HOST_LOCK,'sh','-c','echo ready; read answer'],{env});
  await once(holder.stdout,'data');
  const before=posts;
  assert.match((await run()).output,/another maintenance job/);assert.equal(posts,before);
  holder.stdin.end('done\n');await once(holder,'close');
  assert.equal((await run()).code,0);
  assert.equal(await readFile(join(work,'reboots'),'utf8'),'reboot\n');
  assert.equal(deletes,3,'successful reboot must retain admission until shutdown');
  await server.close();
  const unreachable=await run();assert.notEqual(unreachable.code,0);
  assert.equal(await readFile(join(work,'reboots'),'utf8'),'reboot\n');
  console.log('Reboot journey passed: required flag, window, busy work, inhibitors, failure cleanup and orderly request.');
}finally{server.close();await rm(work,{recursive:true,force:true});}
