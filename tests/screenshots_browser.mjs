// Browser journeys cover mocked API boundaries and real Chromium tab capture.
import assert from 'node:assert/strict';
import {readFile,mkdir} from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.INFUSER_CHROMIUM,args:['--auto-accept-this-tab-capture','--enable-usermedia-screen-capturing']});
const root=new URL('../',import.meta.url),out=new URL('.local/screenshots-review/',root);
await mkdir(out,{recursive:true});
const image='data:image/webp;base64,'+(await readFile(new URL('docs/design-explorations/card-art-guide/firefly-drift.webp',root))).toString('base64');
const fixture=`<meta name="viewport" content="width=device-width,initial-scale=1"><link rel="stylesheet" href="/assets/clubhouse.css"><link rel="stylesheet" href="/assets/themes/nightglass.css"><header><div id="friends"><span data-player-id="me">Pilot</span></div></header><script type="module" src="/fixture.js"></script>`;
const setup=`import {initChat} from '/assets/chat.js';const api=async(path,options)=>{const r=await fetch(path,options);if(!r.ok)throw Object.assign(Error('Screenshot expired'),{status:r.status});return r.json();};initChat({me:{player_id:'me'},api,profileAvatar:()=>document.createElement('span')});`;
try{
 for(const width of [1280,390,320]){
  const context=await browser.newContext({viewport:{width,height:800}});let posts=[],messages=[];
  await context.route('https://shots.test/**',async route=>{
   const r=route.request(),path=new URL(r.url()).pathname;
   if(path==='/api/chat'){
    if(r.method()==='POST'){const data=r.postDataJSON();posts.push(data);messages.push({...data,id:messages.length+1,author:{id:'me',name:'Pilot'},created:Date.now()/1000});return route.fulfill({json:{ok:true}});}
    return route.fulfill({json:{messages}});
   }
   if(path==='/api/screenshots')return route.fulfill({json:{screenshots:[{id:'shot'}]}});
   if(path.startsWith('/api/screenshots/'))return route.fulfill({json:{image}});
   if(path==='/fixture.js')return route.fulfill({body:setup,contentType:'text/javascript'});
   if(path.startsWith('/assets/'))return route.fulfill({body:await readFile(new URL('web/'+path.slice(8),root)),contentType:path.endsWith('.js')?'text/javascript':path.endsWith('.json')?'application/json':'text/css'});
   return route.fulfill({body:fixture,contentType:'text/html'});
  });
  const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));await page.goto('https://shots.test');
  await page.locator('#chat-toggle').click();await page.locator('#chat-text').fill('Great flight!');
  async function attach(){await page.locator('#chat-emoji-toggle').click();await page.getByRole('tab',{name:'Screenshots',exact:true}).click();const tile=page.locator('.screenshot-picker .screenshot-tile');await tile.locator('img').evaluate(async img=>{if(!img.complete)await new Promise(r=>img.onload=r);});await tile.click();}
  await attach();assert.equal(posts.length,0);assert.equal(await page.locator('#chat-text').inputValue(),'Great flight!');
  await page.getByRole('button',{name:'Remove screenshot attachment'}).click();assert(await page.locator('.screenshot-draft').isHidden());await attach();
  await page.waitForFunction(()=>document.querySelector('.screenshot-draft img')?.naturalWidth>0);
  await page.screenshot({path:new URL(`draft-${width}.png`,out).pathname});
  await page.locator('#chat-send').click();await page.locator('.chat-message .screenshot-tile').waitFor();assert.equal(posts[0].screenshot,'shot');
  await page.locator('.chat-message .screenshot-tile').click();await page.locator('.screenshot-viewer img').waitFor();assert(await page.locator('#chat-panel').isVisible());
  const box=await page.locator('.screenshot-viewer').boundingBox();assert(box.x>=0&&box.x+box.width<=width+1);await page.screenshot({path:new URL(`viewer-${width}.png`,out).pathname});
  await page.keyboard.press('Escape');assert(await page.locator('.screenshot-viewer').isHidden());assert(await page.locator('#chat-panel').isVisible());
  await attach();await page.locator('#chat-send').click();await page.waitForFunction(()=>document.querySelectorAll('.chat-message').length===2);assert.equal(posts[1].text,'','image-only message');
  assert.deepEqual(errors,[]);await context.close();
 }
 // Fake only the browser permission/crop boundary; encode a real video frame.
 const context=await browser.newContext();let uploads=[];
 await context.route('https://capture.test/**',async route=>{
  const path=new URL(route.request().url()).pathname;
  if(path==='/_infuser/screenshots'){uploads.push(route.request().postDataJSON());return route.fulfill({json:{saved:true}});}
  if(path==='/capture.js')return route.fulfill({body:await readFile(new URL('web/screenshot-capture.js',root)),contentType:'text/javascript'});
  return route.fulfill({contentType:'text/html',body:`<button id="capture">Screenshot</button><main id="viewport"></main><p id="status"></p><script type="module">import {initScreenshotCapture} from '/capture.js';initScreenshotCapture({button:document.querySelector('button'),viewport:document.querySelector('main'),message:text=>document.querySelector('p').textContent=text});</script>`});
 });
 const page=await context.newPage();await page.goto('https://capture.test');
 await page.evaluate(()=>{
  window.mode='ok';window.stopped=0;window.options=[];
  window.CropTarget={fromElement:async element=>{if(element.id!=='viewport')throw Error('Wrong crop');return {};}};
  navigator.mediaDevices.getDisplayMedia=async options=>{
   window.options.push(options);if(mode==='cancel')throw new DOMException('Cancelled','NotAllowedError');
   const canvas=document.createElement('canvas');canvas.width=960;canvas.height=540;const ctx=canvas.getContext('2d');ctx.fillStyle='#157';ctx.fillRect(0,0,960,540);
   const stream=canvas.captureStream(30),track=stream.getVideoTracks()[0],stop=track.stop.bind(track);
   track.stop=()=>{stopped++;clearInterval(window.paint);stop();};track.cropTo=async()=>{if(mode==='wrong')throw Error('Wrong tab');};
   window.paint=setInterval(()=>ctx.fillRect(0,0,960,540),30);return stream;
  };
 });
 await page.locator('button').click();await page.getByText(/Screenshot saved/).waitFor();assert.equal(uploads.length,1);
 assert(await page.evaluate(()=>stopped===1&&options[0].audio===false));
 const size=await page.evaluate(async payload=>{const img=await createImageBitmap(await (await fetch('data:image/webp;base64,'+payload.image)).blob());const thumb=await createImageBitmap(await(await fetch('data:image/webp;base64,'+payload.thumbnail)).blob());return [img.width,img.height,thumb.width,thumb.height];},uploads[0]);assert.deepEqual(size,[960,540,320,180]);
 await page.evaluate(()=>mode='wrong');await page.locator('button').click();await page.getByText(/Choose this game/).waitFor();assert.equal(uploads.length,1);assert.equal(await page.evaluate(()=>stopped),2);
 await page.evaluate(()=>mode='cancel');await page.locator('button').click();await page.getByText('Screenshot not saved.',{exact:true}).waitFor();assert.equal(uploads.length,1);
 await page.evaluate(()=>delete window.CropTarget);await page.locator('button').click();await page.getByText(/not available in this browser/).waitFor();assert.equal(uploads.length,1);
 await context.close();
 // Real tab capture: Chromium approves only this isolated test tab. The game
 // iframe renders WebGL with its default buffer settings and no capture hook.
 const real=await browser.newContext({viewport:{width:1200,height:800}});let captured;
 await real.route('https://real-capture.test/**',async route=>{
  const path=new URL(route.request().url()).pathname;
  if(path==='/_infuser/screenshots'){captured=route.request().postDataJSON();return route.fulfill({json:{saved:true}});}
  if(path==='/capture.js')return route.fulfill({body:await readFile(new URL('web/screenshot-capture.js',root)),contentType:'text/javascript'});
  if(path==='/game')return route.fulfill({contentType:'text/html',body:`<style>body{margin:0}</style><canvas width="960" height="540"></canvas><script>const gl=document.querySelector('canvas').getContext('webgl');function draw(){gl.clearColor(0,0.5,0.25,1);gl.clear(gl.COLOR_BUFFER_BIT);requestAnimationFrame(draw)}draw();</script>`});
  return route.fulfill({contentType:'text/html',headers:{'Content-Security-Policy':"default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'unsafe-inline'; frame-src 'self'; connect-src 'self' data:; media-src blob:"},body:`<button>Screenshot</button><p></p><main style="width:960px;height:540px"><iframe src="/game" style="display:block;border:0;width:100%;height:100%"></iframe></main><script type="module">import {initScreenshotCapture} from '/capture.js';initScreenshotCapture({button:document.querySelector('button'),viewport:document.querySelector('main'),message:text=>document.querySelector('p').textContent=text});</script>`});
 });
 const actual=await real.newPage();await actual.goto('https://real-capture.test');await actual.locator('button').click();await actual.getByText(/Screenshot saved/).waitFor();
 const frame=await actual.evaluate(async data=>{const image=await createImageBitmap(await(await fetch('data:image/webp;base64,'+data.image)).blob());const canvas=document.createElement('canvas');canvas.width=image.width;canvas.height=image.height;const ctx=canvas.getContext('2d');ctx.drawImage(image,0,0);return {width:canvas.width,height:canvas.height,pixel:[...ctx.getImageData(400,200,1,1).data]};},captured);
 assert.equal(frame.width,960);assert.equal(frame.height,540);assert(frame.pixel[0]<15&&frame.pixel[1]>80&&frame.pixel[2]>30,'WebGL iframe rendered into capture, not a black canvas');
 await real.close();
 console.log('PASS: screenshot drafts, removal, chat attachments, image-only messages, viewer and mobile widths; original resolution, thumbnails, stopped capture, no audio, cancellation and crop failure; real Chromium tab capture/cropping of a WebGL iframe with test-tab permission auto-approved.');
}finally{await browser.close();}
