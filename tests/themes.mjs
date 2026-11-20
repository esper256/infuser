import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source=readFileSync(new URL('../web/themes.js',import.meta.url),'utf8');
function boot(saved,{blocked=false,code=source,loading=false,sheets}={}) {
  const events=new Map(), links=[], written=[];
  function makeLink(){
    const listeners=new Map();
    return {
      rel:'',href:'',dataset:{},sheet:null,blocking:'',
      setAttribute(name,value){if(name==='blocking')this.blocking=value;},
      addEventListener(type,fn,options){
        const list=listeners.get(type)||[];
        list.push({fn,once:!!(options&&options.once)});
        listeners.set(type,list);
      },
      dispatchEvent(event){
        const list=listeners.get(event.type)||[];
        const stay=[];
        for(const item of list){item.fn(event);if(!item.once)stay.push(item);}
        listeners.set(event.type,stay);
      },
      remove(){const index=links.indexOf(this);if(index>=0)links.splice(index,1);},
    };
  }
  const document={
    readyState:loading?'loading':'interactive',
    documentElement:{dataset:{}},
    head:{appendChild(node){links.push(node);}},
    createElement:()=>makeLink(),
    querySelector(selector){
      const id=/data-infuser-theme="([^"]+)"/.exec(selector);
      return id?links.find(link=>link.dataset.infuserTheme===id[1])||null:null;
    },
    write(html){
      written.push(html);
      const href=/href="([^"]*)"/.exec(html), id=/data-infuser-theme="([^"]*)"/.exec(html);
      if(!href||!id)return;
      const link=makeLink();
      link.href=href[1];link.dataset.infuserTheme=id[1];links.push(link);
    },
    addEventListener(){},
  };
  const values=new Map(saved===undefined?[]:[['infuser.theme',saved]]);
  const localStorage={getItem:key=>{if(blocked)throw Error('blocked');return values.get(key)??null;},setItem:(key,value)=>{if(blocked)throw Error('blocked');values.set(key,value);}};
  const window={addEventListener:(name,fn)=>events.set(name,fn),dispatchEvent:()=>{}};
  const sandbox={window,document,localStorage,Event:class {}};
  if(sheets)sandbox.InfuserThemeSheets=sheets;
  vm.runInNewContext(code,sandbox);
  return {
    themes:window.InfuserThemes,values,links,written,
    storage:event=>events.get('storage')(event),
    load(id){
      const link=links.find(item=>item.dataset.infuserTheme===id);
      assert.ok(link,id);
      link.sheet={};
      link.dispatchEvent({type:'load'});
    },
  };
}
test('baseline stays default; a saved candidate restores without adopting it',()=>{
  assert.equal(boot().themes.current,'nightglass');
  const {themes}=boot('cartridge-club');
  assert.equal(themes.current,'cartridge-club');
  assert.equal(themes.catalog.find(t=>t.id===themes.defaultId).status,'stable');
  assert.equal(themes.catalog.find(t=>t.id==='cartridge-club').status,'stable');
  const funny=boot('funny-pages');
  assert.equal(funny.themes.current,'funny-pages');
  assert.equal(funny.themes.catalog.find(t=>t.id==='funny-pages').status,'preview');
  assert.equal(funny.themes.defaultId,'nightglass');
});
test('funny pages rules are scoped to that theme',()=>{
  const html=readFileSync(new URL('../web/index.html',import.meta.url),'utf8');
  const profiles=readFileSync(new URL('../web/profiles.html',import.meta.url),'utf8');
  const css=readFileSync(new URL('../web/themes/funny-pages.css',import.meta.url),'utf8').replace(/\/\*[\s\S]*?\*\//g,'');
  for(const page of [html,profiles]){
    assert.equal(page.includes('funny-pages.css'),false);
    const night=page.indexOf('/assets/themes/nightglass.css');
    const shared=page.indexOf('/assets/clubhouse.css');
    const script=page.indexOf('/assets/themes.js');
    assert.ok(night>=0&&shared>night&&script>shared);
  }
  function walk(sheet){
    let i=0;
    while(i<sheet.length){
      const start=i;
      while(i<sheet.length&&sheet[i]!=='{')i++;
      const prelude=sheet.slice(start,i).trim();
      if(i>=sheet.length)break;
      i++;
      const innerStart=i;
      let depth=1;
      while(i<sheet.length&&depth){
        if(sheet[i]==='{')depth++;
        else if(sheet[i]==='}')depth--;
        if(depth)i++;
      }
      const inner=sheet.slice(innerStart,i);
      i++;
      if(!prelude)continue;
      if(/^@(media|container|supports|layer)\b/.test(prelude))walk(inner);
      else if(/^@font-face\b/.test(prelude))continue;
      else if(prelude.startsWith('@'))assert.fail('unexpected at-rule '+prelude);
      else assert.ok(prelude.includes('[data-theme="funny-pages"]'),prelude);
    }
  }
  walk(css);
});
test('field notes rules are scoped to that theme',()=>{
  const html=readFileSync(new URL('../web/index.html',import.meta.url),'utf8');
  const profiles=readFileSync(new URL('../web/profiles.html',import.meta.url),'utf8');
  const css=readFileSync(new URL('../web/themes/field-notes.css',import.meta.url),'utf8').replace(/\/\*[\s\S]*?\*\//g,'');
  for(const page of [html,profiles]) assert.equal(page.includes('field-notes.css'),false);
  assert.equal(boot('field-notes').themes.current,'field-notes');
  assert.equal(boot().themes.catalog.find(t=>t.id==='field-notes').status,'preview');
  function walk(sheet){
    let i=0;
    while(i<sheet.length){
      const start=i;
      while(i<sheet.length&&sheet[i]!=='{')i++;
      const prelude=sheet.slice(start,i).trim();
      if(i>=sheet.length)break;
      i++;
      const innerStart=i;
      let depth=1;
      while(i<sheet.length&&depth){
        if(sheet[i]==='{')depth++;
        else if(sheet[i]==='}')depth--;
        if(depth)i++;
      }
      const inner=sheet.slice(innerStart,i);
      i++;
      if(!prelude)continue;
      if(/^@(media|container|supports|layer)\b/.test(prelude))walk(inner);
      else if(/^@font-face\b/.test(prelude))continue;
      else if(prelude.startsWith('@'))assert.fail('unexpected at-rule '+prelude);
      else assert.ok(prelude.includes('[data-theme="field-notes"]'),prelude);
    }
  }
  walk(css);
});
test('selection persists and other tabs can change or clear it',async()=>{
  const {themes,values,storage,load}=boot();
  const pending=themes.select('cartridge-club');
  assert.equal(themes.current,'nightglass');
  load('cartridge-club');
  await pending;
  assert.equal(values.get('infuser.theme'),'cartridge-club');
  assert.equal(themes.current,'cartridge-club');
  values.set('infuser.theme','nightglass');await storage({key:'infuser.theme'});assert.equal(themes.current,'nightglass');
  const again=themes.select('cartridge-club');load('cartridge-club');await again;
  values.clear();await storage({key:null});assert.equal(themes.current,'nightglass');
});
test('unknown and retired choices fall back rather than selecting arbitrary CSS',()=>{
  assert.equal(boot('https://example.com/theme.css').themes.current,'nightglass');
  const code=source.replace("name: 'Cartridge Club', status: 'stable'","name: 'Cartridge Club', status: 'retired'");
  const {themes}=boot('cartridge-club',{code});
  assert.equal(themes.current,'nightglass');themes.select('cartridge-club');assert.equal(themes.current,'nightglass');
});
test('browser chrome token is copied onto the theme-color meta',()=>{
  const meta={content:''};
  const document={
    documentElement:{dataset:{}},
    readyState:'loading',
    querySelector:selector=>selector==='meta[name="theme-color"]'?meta:null,
    addEventListener(){},
  };
  const values=new Map();
  const localStorage={getItem:key=>values.get(key)??null,setItem:(key,value)=>values.set(key,value)};
  const window={addEventListener(){},dispatchEvent(){}};
  vm.runInNewContext(source,{window,document,localStorage,getComputedStyle:()=>({getPropertyValue:()=>' #0c1017 '}),Event:class {}});
  assert.equal(meta.content,'#0c1017');
});
test('blocked storage does not break selection for this visit',async()=>{
  const {themes,load}=boot(undefined,{blocked:true});
  assert.equal(themes.current,'nightglass');
  const pending=themes.select('cartridge-club');
  assert.equal(themes.current,'nightglass');
  load('cartridge-club');
  await pending;
  assert.equal(themes.current,'cartridge-club');
});
test('a nightglass load requests no other theme css and switching loads that sheet',async()=>{
  const read=name=>readFileSync(new URL(name,import.meta.url),'utf8');
  const html=read('../web/index.html'), profiles=read('../web/profiles.html'), desktop=read('../web/desktop.html');
  const worker=read('../web/sw.js'), member=read('../src/portal/member_link.rs');
  for(const page of [html,profiles,desktop,member]){
    assert.equal(page.includes('cartridge-club.css'),false);
    assert.equal(page.includes('funny-pages.css'),false);
    assert.equal(page.includes('field-notes.css'),false);
    assert.ok(page.includes('/assets/themes/nightglass.css'));
  }
  assert.match(worker,/request\.mode!=='navigate'/);
  assert.doesNotMatch(worker,/themes\/.+\.css/);
  const night=boot(undefined,{loading:true});
  assert.equal(night.themes.current,'nightglass');
  assert.deepEqual(night.written,[]);
  assert.deepEqual(night.links,[]);
  const saved=boot('funny-pages',{loading:true});
  assert.equal(saved.themes.current,'funny-pages');
  assert.equal(saved.written.length,1);
  assert.match(saved.written[0],/href="\/assets\/themes\/funny-pages\.css"/);
  assert.match(saved.written[0],/data-infuser-theme="funny-pages"/);
  assert.match(saved.written[0],/blocking="render"/);
  assert.doesNotMatch(saved.written[0],/cartridge-club|field-notes|nightglass/);
  const {themes,links,load,values}=boot();
  assert.equal(themes.current,'nightglass');
  const pending=themes.select('cartridge-club');
  assert.equal(themes.current,'nightglass');
  assert.equal(values.has('infuser.theme'),false);
  assert.deepEqual(links.map(link=>link.href),['/assets/themes/cartridge-club.css']);
  links[0].dispatchEvent({type:'error'});
  await assert.rejects(pending);
  assert.equal(themes.current,'nightglass');
  assert.deepEqual(links.map(link=>link.href),[]);
  const choice=themes.select('field-notes');
  assert.deepEqual(links.map(link=>link.href),['/assets/themes/field-notes.css']);
  load('field-notes');
  await choice;
  assert.equal(themes.current,'field-notes');
  assert.equal(values.get('infuser.theme'),'field-notes');
  await themes.select('nightglass');
  assert.equal(themes.current,'nightglass');
  assert.deepEqual(links.map(link=>link.href),['/assets/themes/field-notes.css']);
  const registry=boot(undefined,{sheets:[
    {id:'nightglass',href:'/assets/themes/nightglass.css?v=base'},
    {id:'cartridge-club',href:'/assets/themes/cartridge-club.css?v=cart'},
    {id:'field-notes',href:'/assets/themes/field-notes.css?v=field'},
  ]});
  const fingerprinted=registry.themes.select('cartridge-club');
  assert.equal(registry.themes.current,'nightglass');
  assert.deepEqual(registry.links.map(link=>link.href),['/assets/themes/cartridge-club.css?v=cart']);
  registry.load('cartridge-club');
  await fingerprinted;
  assert.equal(registry.themes.current,'cartridge-club');
  await assert.rejects(()=>registry.themes.select('funny-pages'));
  assert.equal(registry.themes.current,'cartridge-club');
  assert.equal(registry.links.length,1);
});
test('a slower theme sheet does not paint over a newer choice',async()=>{
  const {themes,load,values}=boot();
  const first=themes.select('cartridge-club');
  const second=themes.select('funny-pages');
  load('cartridge-club');
  await first;
  assert.equal(themes.current,'nightglass');
  assert.equal(values.has('infuser.theme'),false);
  load('funny-pages');
  await second;
  assert.equal(themes.current,'funny-pages');
  assert.equal(values.get('infuser.theme'),'funny-pages');
});
