// Opt-in browser regression: INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs
// node tests/library_art_journey.mjs (no running server or account required).
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const browser=await chromium.launch({headless:true,...(process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
const page=await browser.newPage({viewport:{width:1200,height:900}});
const errors=[];page.on('pageerror',error=>errors.push(error.message));
const cover=await fs.readFile(new URL('../docs/design-explorations/card-art-guide/firefly-drift.webp',import.meta.url));
let coverRequested,releaseCover;
const coverResponse=new Promise(resolve=>{releaseCover=resolve;});
const pendingCoverRequest=new Promise(resolve=>{coverRequested=resolve;});
await page.route('https://library.test/**',async route=>{
  const path=new URL(route.request().url()).pathname;
  if(path==='/api/me')return route.fulfill({status:401,json:{error:'Test renderer without a member session'}});
  if(path==='/art/pending') {coverRequested();await coverResponse;return route.fulfill({contentType:'image/webp',body:cover});}
  if(path==='/art/broken')return route.fulfill({status:404,body:''});
  if(path.startsWith('/art/'))return route.fulfill({contentType:'image/webp',body:cover});
  const file=path==='/library'?'index.html':path.startsWith('/assets/')?path.slice(8):null;
  if(file&&!file.includes('..')){
    try{
      let body=await fs.readFile(new URL('../web/'+file,import.meta.url));
      if(file==='clubhouse.js'){
        if(process.env.INFUSER_CLUBHOUSE_SOURCE)body=await fs.readFile(process.env.INFUSER_CLUBHOUSE_SOURCE);
        body=body.toString()+`\nwindow.testLibrary={render(data){me={admin:false,avatars:[]};library=data;document.getElementById('clubhouse').hidden=false;document.getElementById('welcome').hidden=true;renderGames();},openDetails(){openGameDetail(library.apps[0]);}};`;
      }
      return route.fulfill({body,contentType:file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':'text/html'});
    }catch{}
  }
  return route.fulfill({status:404,body:''});
});
const game=(id,art='/art/one')=>({id,title:id,card_art_url:art,play_modes:['solo'],tags:[],players:[],reactions:[]});
const render=apps=>page.evaluate(apps=>window.testLibrary.render({apps,tags:[]}),apps);
const art=page.locator('[data-app="firefly"] .game-art');
try{
  await page.goto('https://library.test/library');
  await page.waitForFunction(()=>window.testLibrary);
  await render([game('firefly'),game('other')]);
  await page.waitForFunction(()=>document.querySelectorAll('.has-game-art').length===2);
  await page.evaluate(()=>{
    window.originalCard=document.querySelector('[data-app="firefly"]');
    window.originalArt=originalCard.querySelector('.game-art');
    window.originalImage=originalArt.querySelector('img');
    window.coverRemovals=[];
    new MutationObserver(records=>{for(const record of records)for(const node of record.removedNodes)if(node===originalImage||node.contains?.(originalImage))coverRemovals.push(node.nodeName);}).observe(document.getElementById('games'),{childList:true,subtree:true});
  });
  for(let i=0;i<5;i++)await render([{...game('firefly'),title:'Updated title',players:[{name:'Player'}],reactions:[{emoji:'⭐'}]},game('other')]);
  assert.equal(await page.evaluate(()=>document.querySelector('[data-app="firefly"] .game-art img')===originalImage),true,'refresh must preserve the decoded image');
  assert.deepEqual(await page.evaluate(()=>coverRemovals),[],'refresh must not detach the loaded cover');
  assert.equal(await art.getAttribute('class'),'game-art game-mark has-game-art');
  assert.equal(await art.evaluate(node=>getComputedStyle(node,'::after').content),'none','fallback must stay hidden');
  assert.equal(await page.locator('[data-app="firefly"] h2').textContent(),'Updated title');
  assert.equal(await page.locator('[data-app="firefly"] .game-live-menu .game-player-name').textContent(),'Player');
  assert.equal(await page.locator('.play-menu .players').count(),0,'Play menus do not duplicate player lists');

  await render([game('other'),game('firefly')]);
  assert.deepEqual(await page.locator('.game').evaluateAll(nodes=>nodes.map(node=>node.dataset.app)),['other','firefly']);
  assert.equal(await art.locator('img').evaluate(node=>node===originalImage),true,'reordering keeps the image');
  await render([{...game('firefly'),updated:10},{...game('other'),updated:20}]);
  assert.deepEqual(await page.locator('.game').evaluateAll(nodes=>nodes.map(node=>node.dataset.app)),['other','firefly'],'newest updates come first regardless of API order');
  await render([{...game('other'),updated:20},{...game('firefly'),updated:30}]);
  assert.deepEqual(await page.locator('.game').evaluateAll(nodes=>nodes.map(node=>node.dataset.app)),['firefly','other'],'a newly updated game moves to the front');
  assert.equal(await art.locator('img').evaluate(node=>node===originalImage),true,'sorting by update retains loaded cover art');
  await render([game('firefly','/art/two')]);
  await page.waitForFunction(()=>document.querySelector('.game .has-game-art img')?.getAttribute('src')==='/art/two');
  assert.equal(await art.locator('img').evaluate(node=>node===originalImage),false,'changed URL loads new art');
  assert.equal(await page.locator('.game').count(),1,'unlisted cards disappear');

  await render([game('firefly','/art/pending')]);
  await page.waitForFunction(()=>document.querySelector('.game .game-art img')?.getAttribute('src')==='/art/pending');
  await page.evaluate(()=>window.testLibrary.openDetails());
  const detailArt=page.locator('.detail-art');
  for(const theme of ['nightglass','cartridge-club']){
    await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    for(const surface of [art,detailArt]){
      assert.equal(await surface.evaluate(node=>getComputedStyle(node,'::before').content),'none','loading art must not flash the missing-art decoration');
      assert.equal(await surface.evaluate(node=>getComputedStyle(node,'::after').content),'none','loading art must not flash the missing-art decoration');
      assert.equal(await surface.evaluate(node=>getComputedStyle(node).backgroundImage),'none','loading art uses a quiet solid background');
      assert.notEqual(await surface.evaluate(node=>getComputedStyle(node).backgroundColor),'rgba(0, 0, 0, 0)','loading art keeps a dark background');
    }
  }
  await page.evaluate(()=>window.pendingImage=document.querySelector('.game .game-art img'));
  await render([game('firefly','/art/pending')]);
  assert.equal(await art.locator('img').evaluate(node=>node===pendingImage),true,'refresh must retain an in-flight image');
  await pendingCoverRequest;releaseCover();
  await page.waitForFunction(()=>document.querySelector('.detail-art').classList.contains('has-game-art'));
  await page.locator('#close-game-detail').click();
  await page.waitForFunction(()=>document.querySelector('.game .game-art').classList.contains('has-game-art'));
  await render([game('firefly',null)]);
  assert.equal(await art.locator('img').count(),0);
  assert.notEqual(await art.evaluate(node=>getComputedStyle(node,'::after').content),'none','removed cover uses fallback');
  await render([game('firefly','/art/broken')]);
  await page.waitForFunction(()=>!document.querySelector('.game .game-art img'));
  assert.notEqual(await art.evaluate(node=>getComputedStyle(node,'::after').content),'none','failed image uses fallback');
  await render([game('firefly')]);
  await page.waitForFunction(()=>document.querySelector('.game .has-game-art'));

  await page.locator('#game-search').fill('not-found');
  assert.equal(await page.locator('.game').count(),0);
  assert.equal(await page.locator('#games h2').textContent(),'No matches');
  await page.locator('#game-search').fill('');
  assert.equal(await page.locator('.game').count(),1);
  await page.locator('.game .play-menu summary').click();
  await render([]);
  assert.equal(await page.locator('.game').count(),1,'open menu remains usable during refresh');
  await page.locator('.game .play-menu summary').click();
  await render([]);
  assert.equal(await page.locator('#games h2').textContent(),'No games yet');
  assert.deepEqual(errors,[]);
  console.log('PASS: cards and details stay dark while covers load in both themes; loaded and pending covers survive refresh; metadata, ordering, replacement, fallback, search and menu behavior remain correct.');
}finally{await browser.close();}
