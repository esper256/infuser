// Opt-in browser journey with mocked API data; no server or account required.
// INFUSER_PLAYWRIGHT=/path/to/playwright/index.mjs node tests/tag_overflow_journey.mjs
// Set INFUSER_TOUCH=1 to also exercise touch opening and coarse-pointer styles.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT||'playwright');
const root=new URL('../',import.meta.url);
const browser=await chromium.launch({headless:true,...(process.env.INFUSER_CHROMIUM?{executablePath:process.env.INFUSER_CHROMIUM}:{})});
const context=await browser.newContext({hasTouch:process.env.INFUSER_TOUCH==='1'});const page=await context.newPage();
const errors=[];page.on('pageerror',e=>errors.push(e.message));
const me={player_id:'self',name:'Alex',admin:false,avatar:'circuit-fox',avatars:[{id:'circuit-fox',name:'Circuit Fox'},{id:'moss-knight',name:'Moss Knight'}]};
let friends=[{id:'a',name:'Rowan',avatar:'moss-knight'},{id:'b',name:'Sam',avatar:'circuit-fox'},{id:'c',name:'Quinn',avatar:'moss-knight'}];
let tagCatalog=['Arcade','Chill','Cooperative','Adventure','Puzzle','Racing','Strategy','Something with a very long tag label'].map((label,i)=>({id:'tag-'+i,label,count:1}));
const game={id:'firefly',title:'Firefly Drift',card_art_url:'/art',play_modes:['solo'],tags:tagCatalog.map(t=>t.id),can_edit_tags:true,players:[],reactions:[],updated:1};
await page.route('https://library.test/**',async route=>{
 const path=new URL(route.request().url()).pathname;
 if(path==='/api/me')return route.fulfill({json:me});
 if(path==='/api/library')return route.fulfill({json:{apps:[game],friends,tags:tagCatalog}});
 if(path==='/api/chat')return route.fulfill({json:{messages:[]}});
 if(path==='/api/client-state')return route.fulfill({json:{game_open:true,version:'test'}});
 if(path==='/api/profile'&&route.request().method()==='GET')return route.fulfill({json:{users:[]}});
 if(path==='/api/profile'){me.avatar=route.request().postDataJSON().avatar;return route.fulfill({json:{avatar:me.avatar}});}
 let file=path==='/library'?'web/index.html':path==='/art'?'docs/design-explorations/card-art-guide/firefly-drift.webp':path.startsWith('/assets/')?'web/'+path.slice(8):null;
 const avatar=/^\/assets\/avatars\/([^/]+)\/(\d+)\.webp$/.exec(path);if(avatar)file=`assets/avatars/${avatar[1]}-${avatar[2]}.webp`;
 if(file)try{
  let body=await fs.readFile(new URL(file,root));
  if(file==='web/clubhouse.js')body=body.toString()+'\nwindow.refreshLibrary=refresh;';
  return route.fulfill({body,contentType:file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':file.endsWith('.webp')?'image/webp':'text/html'});
 }catch{}
 return route.fulfill({status:404,body:''});
});
try{
 await page.setViewportSize({width:1200,height:850});await page.goto('https://library.test/library');await page.waitForFunction(()=>window.refreshLibrary);
 await page.waitForFunction(()=>document.querySelector('#tag-filters .tag-more:not([hidden])'));
 async function verifyRows(){
  const result=await page.locator('.tag-row').evaluateAll(rows=>rows.map(row=>{
   const buttons=[...row.querySelector('.tag-strip').children];
   const textVisible=buttons.every((button,i)=>{const range=document.createRange();range.selectNodeContents(button);return !buttons[i+1]||range.getBoundingClientRect().right<=buttons[i+1].getBoundingClientRect().left+1;});
   return {width:row.clientWidth,scroll:row.scrollWidth,textVisible};
  }));
  for(const row of result){assert.ok(row.scroll<=row.width+1,JSON.stringify(row));assert.ok(row.textVisible,'overlap must not cover text');}
 }
 for(const theme of ['nightglass','cartridge-club'])for(const width of [1200,650,390,320]){
  await page.setViewportSize({width,height:850});await page.evaluate(theme=>InfuserThemes.select(theme),theme);
  await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  await verifyRows();
  const more=page.locator('#tag-filters > .tag-more');await more.hover();
  const panel=page.locator('#tag-filters > .tag-overflow-panel');await panel.waitFor({state:'visible'});
  const bounds=await panel.boundingBox();assert.ok(bounds.x>=15&&bounds.x+bounds.width<=width-15,JSON.stringify(bounds));
  assert.equal(await panel.locator('button').last().textContent(),'Something with a very long tag label');
  await page.evaluate(()=>refreshLibrary());assert.equal(await panel.isVisible(),true,'refresh must preserve open filters');
  await page.mouse.click(5,5);
 }
 if(process.env.INFUSER_TOUCH==='1'){await page.locator('#tag-filters > .tag-more').tap();assert.equal(await page.locator('#tag-filters .tag-overflow-panel').isVisible(),true);await page.mouse.click(5,5);}
 // Overflow entries retain their real filtering handlers.
 await page.locator('#tag-filters > .tag-more').click();
 await page.locator('#tag-filters .tag-overflow-panel button').filter({hasText:'Strategy'}).click();
 assert.equal(await page.locator('#tag-filters button[aria-pressed="true"]').textContent(),'Strategy');
 assert.equal(await page.locator('.game').count(),1);
 await page.locator('#tag-filters .tag-strip button').filter({hasText:'All games'}).click();
 // Keyboard opening, navigation into the list, and Escape focus return.
 await page.mouse.move(1,1);await page.locator('#game-search').focus();
 await page.keyboard.press('Tab');
 await page.locator('#tag-filters > .tag-more').focus();
 await page.locator('#tag-filters .tag-overflow-panel').waitFor({state:'visible'});
 await page.keyboard.press('Tab');
 assert.equal(await page.evaluate(()=>!!document.activeElement.closest('.tag-overflow-panel')),true);
 await page.keyboard.press('Escape');
 assert.equal(await page.locator('#tag-filters .tag-overflow-panel').isVisible(),false);
 assert.equal(await page.evaluate(()=>document.activeElement.classList.contains('tag-more')),true);
 await page.setViewportSize({width:1200,height:850});
 await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
 // Switching from a keyboard-focused list to another row must not reopen the old list.
 await page.locator('#tag-filters > .tag-more').click();
 await page.locator('#tag-filters .tag-overflow-panel button').first().focus();
 await page.locator('.game-tags > .tag-more').hover();
 assert.equal(await page.locator('#tag-filters .tag-overflow-panel').isVisible(),false);
 assert.equal(await page.locator('.game-tags .tag-overflow-panel').isVisible(),true);
 // The card editor remains reachable through overflow.
 await page.locator('.game-tags > .tag-more').click();
 await page.evaluate(()=>refreshLibrary());assert.equal(await page.locator('.game-tags .tag-overflow-panel').isVisible(),true);
 await page.getByRole('button',{name:'＋ Tags',exact:true}).click();
 await page.getByRole('dialog',{name:'Tags: Firefly Drift'}).waitFor();
 await page.locator('#close-tags').click();
 // A wide row with a small catalogue has no overflow; a narrow row preserves spacing.
 tagCatalog=tagCatalog.slice(0,2);game.tags=tagCatalog.map(t=>t.id);
 await page.setViewportSize({width:1200,height:850});await page.evaluate(()=>refreshLibrary());
 assert.equal(await page.locator('#tag-filters > .tag-more').isVisible(),false);
 await page.evaluate(()=>{
  const row=document.getElementById('tag-filters');
  const widths=[...row.querySelector('.tag-strip').children].reduce((sum,b)=>sum+b.getBoundingClientRect().width,0);
  const count=document.getElementById('game-count').getBoundingClientRect().width;
  row.style.width=(widths+count+6)+'px';
 });
 await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
 assert.equal(await page.locator('#tag-filters > .tag-more').isVisible(),true,'overflow should preserve legible spacing');
 assert.ok(parseFloat(await page.locator('#tag-filters > .tag-strip').evaluate(el=>el.style.getPropertyValue('--tag-spacing')))>=0);
 await verifyRows();assert.deepEqual(errors,[]);
 console.log('PASS: no row scrollbars or covered text, legible spacing, overflow filtering/editing, keyboard focus/Escape, refresh preservation, long labels, both themes and 320–1200px widths.');
}finally{await browser.close();}
