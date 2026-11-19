// Operator-tool journeys. Requires Ansible + community.docker; no Docker or root.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtemp, mkdir, writeFile, readFile, readdir, rm, access, chmod} from 'node:fs/promises';
import {tmpdir, userInfo} from 'node:os';
import {resolve, join} from 'node:path';
import {createServer} from 'node:http';
import {setTimeout as delay} from 'node:timers/promises';

const root = resolve(import.meta.dirname, '..');
const ansible = process.env.INFUSER_TEST_ANSIBLE;
assert(ansible, 'Set INFUSER_TEST_ANSIBLE to ansible-playbook; see developer-testing.md.');
const temp = await mkdtemp(join(tmpdir(), 'infuser-ansible-'));
const env = {...process.env, ANSIBLE_CONFIG: join(root, 'deploy/ansible/ansible.cfg'),
  ANSIBLE_LOCAL_TEMP: join(temp, 'ansible-local'), ANSIBLE_REMOTE_TEMP: join(temp, 'ansible-remote')};
const run = (cmd, args, extra = {}) => new Promise((resolve, reject) => {
  const child = spawn(cmd, args, {env: {...env, ...extra}});
  let output = '';
  child.stdout.on('data', b => output += b); child.stderr.on('data', b => output += b);
  child.on('error', reject); child.on('close', code => resolve({code, output}));
});
const waitFor = async file => {
  for(let i=0; i<100; i++){try{await access(file);return;}catch{} await delay(50);}
  throw new Error(`Timed out waiting for ${file}`);
};
const exists = async file => {try{await access(file);return true;}catch{return false;}};
let server;
try {
  const uid = String(userInfo().uid), gid = String(userInfo().gid);
  await mkdir(join(temp, 'files'));
  await writeFile(join(temp, 'public-source'), 'new configuration\n');
  await writeFile(join(temp, 'private-source'), 'PRIVATE_DIFF_CANARY_7c35\n');
  await writeFile(join(temp, 'files/config'), 'old configuration\n');
  await writeFile(join(temp, 'files/private'), 'OLD_PRIVATE_DIFF_CANARY\n');
  await writeFile(join(temp, 'files/other-project'), 'keep me\n');
  const play = [{name:'Disposable host configuration', hosts:'localhost', connection:'local', gather_facts:false,
    vars:{ansible_python_interpreter:process.env.INFUSER_TEST_PYTHON || '/usr/bin/python3',
      infuser_source_dir:root, infuser_manage_units:false, infuser_manage_monitoring:false,
      infuser_manage_service_sources:false, infuser_extra_files:[
        {src:join(temp,'public-source'),dest:join(temp,'files/config'),mode:'0600',owner:uid,group:gid,component:'caddy',sensitive:false},
        {src:join(temp,'private-source'),dest:join(temp,'files/private'),mode:'0600',owner:uid,group:gid,component:'caddy'},
        {src:join(temp,'public-source'),dest:join(temp,'new-directory/config'),mode:'0600',owner:uid,group:gid,component:'caddy',sensitive:false}]},
    roles:[join(root,'deploy/ansible/roles/infuser_host')]}];
  const playPath=join(temp,'play.json'); await writeFile(playPath,JSON.stringify(play));
  let result=await run(ansible,['-i','localhost,',playPath,'--check','--diff']);
  assert.equal(result.code,0,result.output);
  assert(!result.output.includes('PRIVATE_DIFF_CANARY'), 'Secret leaked in preview');
  assert.equal(await readFile(join(temp,'files/config'),'utf8'),'old configuration\n');
  assert.equal(await readFile(join(temp,'files/private'),'utf8'),'OLD_PRIVATE_DIFF_CANARY\n');
  assert(!(await exists(join(temp,'new-directory'))), 'Preview must not create permanent directories');
  result=await run(ansible,['-i','localhost,',playPath,'--diff']); assert.equal(result.code,0,result.output);
  assert(!result.output.includes('PRIVATE_DIFF_CANARY'), 'Secret leaked during apply');
  assert.equal(await readFile(join(temp,'files/config'),'utf8'),'new configuration\n');
  assert.equal(await readFile(join(temp,'files/other-project'),'utf8'),'keep me\n');
  const backups=(await readdir(join(temp,'files'))).filter(p=>p.startsWith('config.') && p.endsWith('~'));
  assert.equal(backups.length,1);
  assert.equal(await readFile(join(temp,'files',backups[0]),'utf8'),'old configuration\n');
  result=await run(ansible,['-i','localhost,',playPath]); assert.equal(result.code,0,result.output);
  assert.match(result.output,/changed=0\s/,'Second apply must be idempotent');
  await chmod(join(temp,'files/config'),0o400);
  result=await run(ansible,['-i','localhost,',playPath]); assert.equal(result.code,0,result.output);
  assert.match(result.output,/Changed components: none/, 'Permission repairs must not trigger image rebuilds or service recreation');
  await writeFile(join(temp,'public-source'),'invalid candidate\n');
  play[0].vars.infuser_extra_files[0].validate='/usr/bin/false %s';
  await writeFile(playPath,JSON.stringify(play));
  result=await run(ansible,['-i','localhost,',playPath]); assert.notEqual(result.code,0);
  assert.equal(await readFile(join(temp,'files/config'),'utf8'),'new configuration\n');
  console.log('Ansible: preview, private diff suppression, apply, backups, validation and idempotence passed.');

  const bin=join(temp,'bin'),runtime=join(temp,'runtime'); await mkdir(bin); await mkdir(runtime);
  const state=join(temp,'state'),ran=join(temp,'ran'); await writeFile(state,'active\n');
  await writeFile(join(bin,'systemctl'),`#!/bin/sh
printf '%s\\n' "$*" >> "$FIXTURE_SYSTEMD_LOG"
if [ "$1" = is-active ]; then grep -qx active "$FIXTURE_HEALTH_STATE"; exit $?; fi
if [ "$2" = infuser-health.timer ]; then
  case "$1" in stop) echo inactive > "$FIXTURE_HEALTH_STATE";; start) echo active > "$FIXTURE_HEALTH_STATE";; esac
  exit 0
fi
if [ "$2" = infuser-health.service ]; then echo loaded; exit 0; fi
case "$1" in
stop) echo inactive > "$FIXTURE_STATE";;
start) echo active > "$FIXTURE_STATE";;
show) cat "$FIXTURE_STATE";;
daemon-reload) :;;
*) exit 9;;
esac
`,{mode:0o755});
  await writeFile(join(temp,'health-state'),'active\n');
  await writeFile(join(bin,'ansible'),`#!/bin/sh\nfor arg in "$@"; do [ "$arg" != --syntax-check ] || exit 0; done\necho ran > "$FIXTURE_RAN"\nif [ "$FIXTURE_HANG" = 1 ]; then exec sleep 60; fi\nexit "\${FIXTURE_EXIT:-0}"\n`,{mode:0o755});
  await writeFile(join(temp,'token'),'a'.repeat(48));
  let busy=false,posts=0;
  server=createServer(async (req,res)=>{
    let body=''; for await(const b of req)body+=b;
    res.setHeader('Content-Type','application/json');
    if(req.url==='/healthz'){res.statusCode=(await readFile(state,'utf8')).trim()==='active'?200:503;res.end('{}');return;}
    assert.equal(req.headers.authorization,'Bearer '+'a'.repeat(48));
    if(req.method==='POST'){
      posts++;assert.deepEqual(JSON.parse(body),{purpose:'ansible'});
      res.statusCode=busy?409:200;res.end(JSON.stringify({lease:{id:'fixture-lease',expires:Math.floor(Date.now()/1000)+60}}));
    }else{assert.equal(req.method,'DELETE');res.end('{}');}
  });
  await new Promise(r=>server.listen(0,'127.0.0.1',r));
  const wrapperEnv={PATH:bin+':'+process.env.PATH,INFUSER_HOST_RUNTIME:runtime,
    INFUSER_HOST_LOCK:join(temp,'maintenance.lock'),INFUSER_CONTROL_TOKEN_FILE:join(temp,'token'),
    INFUSER_API_PORT:String(server.address().port),INFUSER_ANSIBLE_BIN:join(bin,'ansible'),
    FIXTURE_STATE:state,FIXTURE_RAN:ran,FIXTURE_HEALTH_STATE:join(temp,'health-state'),FIXTURE_SYSTEMD_LOG:join(temp,'systemd.log')};
  const worker=join(root,'deploy/ansible/apply.sh'), recovery=join(root,'deploy/ansible/recover.sh');
  busy=true; result=await run('bash',[worker,playPath],wrapperEnv); assert.equal(result.code,75,result.output);
  assert(!(await exists(ran)));assert.equal((await readFile(state,'utf8')).trim(),'active');busy=false;
  result=await run('bash',[worker,playPath],{...wrapperEnv,FIXTURE_EXIT:'23'});
  assert.equal(result.code,23,result.output);assert.equal((await readFile(state,'utf8')).trim(),'active');
  assert(!(await exists(join(runtime,'resume.json'))));
  await rm(ran);
  const child=spawn('bash',[worker,playPath],{env:{...env,...wrapperEnv,FIXTURE_HANG:'1'},detached:true,stdio:'ignore'});
  const closed=new Promise(r=>child.on('close',r));
  await waitFor(ran);
  assert.equal((await readFile(state,'utf8')).trim(),'inactive');
  assert.equal((await readFile(join(temp,'health-state'),'utf8')).trim(),'inactive');
  const before=posts;
  result=await run('bash',[worker,playPath],wrapperEnv);
  assert.equal(result.code,75,result.output);assert.equal(posts,before,'Lock must precede admission');
  process.kill(-child.pid,'SIGKILL');await closed;
  assert(await exists(join(runtime,'resume.json')),'Interrupted worker must retain recovery marker');
  result=await run('bash',[recovery],wrapperEnv);assert.equal(result.code,0,result.output);
  assert.equal((await readFile(state,'utf8')).trim(),'active');
  assert(!(await exists(join(runtime,'resume.json'))));
  result=await run('bash',[worker,playPath],wrapperEnv);assert.equal(result.code,0,result.output);
  assert.equal((await readFile(join(temp,'health-state'),'utf8')).trim(),'active');
  console.log('Maintenance: busy deferral, lock exclusion, failed apply, killed worker recovery and successful apply passed.');
} finally { if(server)await new Promise(r=>server.close(r)); await rm(temp,{recursive:true,force:true}); }
