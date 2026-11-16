// Optional real website/API journey, invoked by visits_journey.rs.
import assert from 'node:assert/strict';
import http from 'node:http';
import {readFile} from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'../services/peerjs/node_modules/playwright/index.mjs');
const [base,app]=process.argv.slice(2),errors=[];
const browser=await chromium.launch({headless:true,...(process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
async function player(who){
  const context=await browser.newContext();
  await context.route('https://games.example.com/**',async route=>{
    const req=route.request(),url=new URL(req.url()),headers={...await req.allHeaders(),host:'games.example.com',cookie:who+'@example.com'};
    delete headers['content-length'];delete headers['accept-encoding'];
    const response=await new Promise((resolve,reject)=>{
      const request=http.request(base+url.pathname+url.search,{method:req.method(),headers},r=>{
        const chunks=[];r.on('data',b=>chunks.push(b));r.on('end',()=>resolve({status:r.statusCode,headers:r.headers,body:Buffer.concat(chunks)}));
      });request.on('error',reject);request.end(req.postDataBuffer()||undefined);
    });
    await route.fulfill(response);
  });
  const page=await context.newPage();page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));
  await page.goto('https://games.example.com/library');await page.locator('#games .game').first().waitFor();
  return page;
}
async function visits(page){await page.locator('#account-trigger').click();await page.getByRole('button',{name:'Visits',exact:true}).click();}
try{
  const operator=await player('operator');await visits(operator);
  await operator.getByLabel('Visit name',{exact:true}).fill('Browser visit');
  await operator.getByLabel('Visitor · Away',{exact:true}).check();
  await operator.getByLabel('Visit game · Away',{exact:true}).check();
  await operator.getByRole('button',{name:'Start visit',exact:true}).click();
  await operator.getByRole('heading',{name:'Browser visit',exact:true}).waitFor();
  await operator.getByRole('button',{name:'Close visits',exact:true}).click();
  const visitor=await player('visitor');await visits(visitor);
  assert.equal(await visitor.getByRole('button',{name:'Start visit',exact:true}).count(),0);
  await visitor.getByRole('button',{name:'Visit',exact:true}).click();
  await visitor.locator('#visit-banner').waitFor();
  assert.equal(await visitor.locator('#games .game').count(),2);
  assert.match(await visitor.locator('#visit-banner').innerText(),/Browser visit.*Visitor/);
  // Visit choice belongs to this tab; a fresh tab starts in the home group.
  const home=await visitor.context().newPage();await home.goto('https://games.example.com/library');await home.locator('#games .game').waitFor();
  assert.equal(await home.locator('#games .game').count(),1);
  await visitor.setViewportSize({width:390,height:844});
  assert(await visitor.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await visitor.getByRole('button',{name:'Return home',exact:true}).click();
  await visitor.locator('#games .game').waitFor();assert.equal(await visitor.locator('#games .game').count(),1);
  await visits(operator);await operator.getByRole('button',{name:'End visit',exact:true}).click();
  await operator.getByRole('heading',{name:'Browser visit',exact:true}).waitFor({state:'detached'});
  // Exercise the real player shell separately with deterministic time. Losing
  // visit authorization removes the game without leaving fullscreen.
  const shell=await browser.newContext(),frame=await shell.newPage();
  let available=true;
  const assets=new Map(await Promise.all(['html','js','css'].map(async ext=>[ext,await readFile(new URL('../web/game-player.'+ext,import.meta.url))])));
  await shell.route('https://game.example.com/**',async route=>{
    const path=new URL(route.request().url()).pathname;
    if(path==='/_infuser/context')return route.fulfill({json:{manifest:{title:'Visit fixture'},library_url:'https://games.example.com/library',visit:{expires_at:Date.now()/1000+3600}}});
    if(path==='/_infuser/api/v1/heartbeat')return available?route.fulfill({json:{visit:{expires_at:Date.now()/1000+3600}}}):route.abort();
    const ext=path==='/_infuser/player'?'html':path.split('.').at(-1);
    if(assets.has(ext))return route.fulfill({contentType:{html:'text/html',js:'text/javascript',css:'text/css'}[ext],body:assets.get(ext)});
    return route.fulfill({contentType:'text/html',body:'<!doctype html><title>Running game</title><p>Game canvas</p>'});
  });
  await frame.clock.install();await frame.goto('https://game.example.com/_infuser/player');
  await frame.locator('#game').waitFor();await frame.getByRole('button',{name:'Enter fullscreen',exact:true}).click();
  assert(await frame.evaluate(()=>!!document.fullscreenElement));
  available=false;await frame.clock.runFor(31000);
  assert(await frame.locator('#game').isHidden());
  assert(await frame.evaluate(()=>!!document.fullscreenElement));
  assert.match(await frame.locator('#status').innerText(),/visit has ended/);
  assert.deepEqual(errors,[]);
  console.log('Visits browser: operator selection, visitor entry, tab isolation, mobile return and early end passed.');
}finally{await browser.close();}
