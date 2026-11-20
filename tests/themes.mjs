import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source=readFileSync(new URL('../web/themes.js',import.meta.url),'utf8');
function boot(saved,{blocked=false,code=source}={}) {
  const events=new Map(), document={documentElement:{dataset:{}}};
  const values=new Map(saved===undefined?[]:[['infuser.theme',saved]]);
  const localStorage={getItem:key=>{if(blocked)throw Error('blocked');return values.get(key)??null;},setItem:(key,value)=>{if(blocked)throw Error('blocked');values.set(key,value);}};
  const window={addEventListener:(name,fn)=>events.set(name,fn),dispatchEvent:()=>{}};
  vm.runInNewContext(code,{window,document,localStorage,Event:class {}});
  return {themes:window.InfuserThemes,values,storage:event=>events.get('storage')(event)};
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
test('funny pages is linked after the other sheets and every rule is scoped',()=>{
  const html=readFileSync(new URL('../web/index.html',import.meta.url),'utf8');
  const profiles=readFileSync(new URL('../web/profiles.html',import.meta.url),'utf8');
  const css=readFileSync(new URL('../web/themes/funny-pages.css',import.meta.url),'utf8').replace(/\/\*[\s\S]*?\*\//g,'');
  for(const page of [html,profiles]){
    const night=page.indexOf('/assets/themes/nightglass.css');
    const shared=page.indexOf('/assets/clubhouse.css');
    const cartridge=page.indexOf('/assets/themes/cartridge-club.css');
    const funny=page.indexOf('/assets/themes/funny-pages.css');
    assert.ok(night>=0&&shared>night&&cartridge>shared&&funny>cartridge);
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
test('field notes is linked last and every rule is scoped',()=>{
  const html=readFileSync(new URL('../web/index.html',import.meta.url),'utf8');
  const profiles=readFileSync(new URL('../web/profiles.html',import.meta.url),'utf8');
  const css=readFileSync(new URL('../web/themes/field-notes.css',import.meta.url),'utf8').replace(/\/\*[\s\S]*?\*\//g,'');
  for(const page of [html,profiles]){
    const funny=page.indexOf('/assets/themes/funny-pages.css');
    const field=page.indexOf('/assets/themes/field-notes.css');
    assert.ok(funny>=0&&field>funny);
  }
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
test('selection persists and other tabs can change or clear it',()=>{
  const {themes,values,storage}=boot();
  themes.select('cartridge-club');assert.equal(values.get('infuser.theme'),'cartridge-club');
  values.set('infuser.theme','nightglass');storage({key:'infuser.theme'});assert.equal(themes.current,'nightglass');
  themes.select('cartridge-club');values.clear();storage({key:null});assert.equal(themes.current,'nightglass');
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
test('blocked storage does not break selection for this visit',()=>{
  const {themes}=boot(undefined,{blocked:true});
  assert.equal(themes.current,'nightglass');themes.select('cartridge-club');assert.equal(themes.current,'cartridge-club');
});
