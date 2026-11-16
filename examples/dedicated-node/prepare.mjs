// Stage one self-contained project, including the current server adapter.
// node examples/dedicated-node/prepare.mjs [.local/dedicated-sdk-project]
import {mkdir,copyFile} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
const source=dirname(fileURLToPath(import.meta.url));
const destination=resolve(process.argv[2]||'.local/dedicated-sdk-project');
if(destination===source)throw Error('Choose a separate staging directory');
for(const path of ['infuser.game.json','public/index.html','public/game.js','public/infuser-optional.js','server/server.js','server/package.json','server/package-lock.json']) {
  await mkdir(dirname(resolve(destination,path)),{recursive:true});await copyFile(resolve(source,path),resolve(destination,path));
}
for(const file of ['connection.mjs','hosting.mjs']) {
  await mkdir(resolve(destination,'server/infuser'),{recursive:true});
  await copyFile(resolve(source,'../../sdk/server',file),resolve(destination,'server/infuser',file));
}
console.log(destination);
