// Trusted recipe code; no project scripts or Dockerfile are executed here.
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
setTimeout(() => process.exit(124), 300_000);
try {
  if (Number(process.versions.node.split('.')[0]) !== Number(process.argv[2])) throw new Error('Configured Node image has the wrong major version');
  fs.mkdirSync('/work/context', {recursive:true});
  fs.cpSync('/input/source', '/work/context/app', {recursive:true});
  process.chdir('/work/context/app');
  const options = {stdio:'inherit', timeout:120_000, env:{
    PATH:process.env.PATH, HOME:'/work', NODE_ENV:'production',
    npm_config_cache:'/work/cache',
    npm_config_userconfig:'/work/empty-user.npmrc',
    npm_config_globalconfig:'/work/empty-global.npmrc',
  }};
  for (const file of fs.readdirSync('/input/deps').sort()) {
    execFileSync('npm', ['cache','add','/input/deps/'+file,'--offline','--ignore-scripts'], options);
  }
  execFileSync('npm', ['ci','--offline','--ignore-scripts','--omit=dev','--engine-strict','--no-audit','--no-fund'], options);
  function readable(path) {
    const stat = fs.lstatSync(path);
    if (stat.isSymbolicLink()) return;
    if (stat.isDirectory()) {
      fs.chmodSync(path, 0o755);
      for (const name of fs.readdirSync(path)) readable(path+'/'+name);
    } else if (stat.isFile()) fs.chmodSync(path, stat.mode & 0o111 ? 0o755 : 0o644);
    else throw new Error('unsupported installed file type');
  }
  readable('/work/context/app');
  fs.writeFileSync('/work/ready', 'ready');
  // Keep tmpfs alive while the controller reads the build context.
} catch (error) {
  console.error(error.message);
  fs.writeFileSync('/work/failed', 'Offline npm installation failed. Check the log, package/lockfile agreement and recipe restrictions.');
}
