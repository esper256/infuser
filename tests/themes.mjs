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
  assert.equal(themes.catalog.find(t=>t.id==='cartridge-club').status,'preview');
});
test('selection persists and other tabs can change or clear it',()=>{
  const {themes,values,storage}=boot();
  themes.select('cartridge-club');assert.equal(values.get('infuser.theme'),'cartridge-club');
  values.set('infuser.theme','nightglass');storage({key:'infuser.theme'});assert.equal(themes.current,'nightglass');
  themes.select('cartridge-club');values.clear();storage({key:null});assert.equal(themes.current,'nightglass');
});
test('unknown and retired choices fall back rather than selecting arbitrary CSS',()=>{
  assert.equal(boot('https://example.com/theme.css').themes.current,'nightglass');
  const code=source.replace("name: 'Cartridge Club', status: 'preview'","name: 'Cartridge Club', status: 'retired'");
  const {themes}=boot('cartridge-club',{code});
  assert.equal(themes.current,'nightglass');themes.select('cartridge-club');assert.equal(themes.current,'nightglass');
});
test('blocked storage does not break selection for this visit',()=>{
  const {themes}=boot(undefined,{blocked:true});
  assert.equal(themes.current,'nightglass');themes.select('cartridge-club');assert.equal(themes.current,'cartridge-club');
});
