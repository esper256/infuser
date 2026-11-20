// Opt-in browser journey against godot_fixture --domain games.localhost.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await chromium.launch({headless:true});
const errors=[];
const context=await browser.newContext({viewport:{width:1000,height:800}});
async function api(path,method='GET',body=''){
 return new Promise((resolve,reject)=>{const r=http.request({hostname:'127.0.0.1',port:17877,path,method,headers:{Host:'games.localhost',Origin:'https://games.localhost',Cookie:'member=admin','Content-Type':'application/json'}},r=>{const chunks=[];r.on('data',c=>chunks.push(c));r.on('end',()=>resolve({status:r.statusCode,body:Buffer.concat(chunks)}));});r.on('error',reject);r.end(body);});
}
await context.route('https://games.localhost/**',async route=>{
 const r=route.request(),path=new URL(r.url()).pathname;
 const files={'/library':'web/index.html','/desktop':'web/desktop.html','/assets/clubhouse.js':'web/clubhouse.js','/assets/clubhouse.css':'web/clubhouse.css','/assets/themes.js':'web/themes.js','/assets/themes/nightglass.css':'web/themes/nightglass.css','/assets/themes/cartridge-club.css':'web/themes/cartridge-club.css','/assets/themes/funny-pages.css':'web/themes/funny-pages.css','/assets/desktop.js':'web/desktop.js'};
 if(files[path])return route.fulfill({body:await fs.readFile(files[path]),contentType:path.endsWith('.js')?'text/javascript':path.endsWith('.css')?'text/css':'text/html'});
 const response=await api(path,r.method(),r.postData()||'');
 return route.fulfill({...response,contentType:path.startsWith('/api/')?'application/json':'image/webp'});
});
try{
 const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
 await page.goto('https://games.localhost/library');
 await page.getByRole('button',{name:'Play',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'Open Dodge the Creeps'});
 await dialog.waitFor();assert.match(await dialog.getByRole('link',{name:'Open Infuser Play'}).getAttribute('href'),/^infuser:\/\/play\?/);
 assert.match(await dialog.getByRole('link',{name:'Install Infuser Play'}).getAttribute('href'),/Infuser-Play\.flatpak$/);
 await fs.mkdir('.local/artifacts',{recursive:true});await page.screenshot({path:'.local/artifacts/native-play.png'});
 await dialog.getByRole('button',{name:'Done'}).click();
 await page.locator('.play-menu summary').click();
 const launch=page.waitForRequest(r=>r.url().includes('/api/native/v1/play/'));
 await page.getByRole('button',{name:'Play in guest mode'}).click();assert.equal((await launch).postDataJSON().guest,true);
 const pair=JSON.parse((await api('/api/native/v1/device/start','POST',JSON.stringify({challenge:'a'.repeat(64)}))).body);
 await page.goto('https://games.localhost/desktop?pair='+pair.request_id);
 await page.getByRole('button',{name:'Connect Infuser Play',exact:true}).click();
 await page.getByText('Connected! Infuser Play will open your game.',{exact:true}).waitFor();
 await page.screenshot({path:'.local/artifacts/native-connect.png'});
 assert.deepEqual(errors,[]);
 console.log('Native website: Play handoff, Flatpak install link, guest request and explicit browser account approval passed.');
}finally{await context.close();await browser.close();}
