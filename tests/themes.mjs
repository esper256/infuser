import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source=readFileSync(new URL('../web/themes.js',import.meta.url),'utf8');
const ordinaryTime = new Date(2026, 6, 1, 12).getTime();
class OrdinaryDate extends Date { constructor(...args) { super(...(args.length ? args : [ordinaryTime])); } }
function boot(saved,{blocked=false,code=source,loading=false,sheets,now=ordinaryTime}={}) {
  const clock={now};
  class CalendarDate extends Date { constructor(...args) { super(...(args.length ? args : [clock.now])); } }
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
  const sandbox={window,document,localStorage,Date:CalendarDate,Event:class {}};
  if(sheets)sandbox.InfuserThemeSheets=sheets;
  vm.runInNewContext(code,sandbox);
  return {
    themes:window.InfuserThemes,values,links,written,
    setDate:date=>{clock.now=date.getTime();},
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
test('castle keep is a preview and every rule is scoped',()=>{
  const html=readFileSync(new URL('../web/index.html',import.meta.url),'utf8');
  const profiles=readFileSync(new URL('../web/profiles.html',import.meta.url),'utf8');
  const css=readFileSync(new URL('../web/themes/castle-keep.css',import.meta.url),'utf8').replace(/\/\*[\s\S]*?\*\//g,'');
  for(const page of [html,profiles]) assert.equal(page.includes('castle-keep.css'),false);
  assert.equal(boot('castle-keep').themes.current,'castle-keep');
  assert.equal(boot().themes.catalog.find(t=>t.id==='castle-keep').status,'preview');
  assert.equal(boot().themes.defaultId,'nightglass');
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
      else assert.ok(prelude.includes('[data-theme="castle-keep"]'),prelude);
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
  vm.runInNewContext(source,{Date:OrdinaryDate,window,document,localStorage,getComputedStyle:()=>({getPropertyValue:()=>' #0c1017 '}),Event:class {}});
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
    assert.equal(page.includes('castle-keep.css'),false);
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
  assert.doesNotMatch(saved.written[0],/cartridge-club|field-notes|nightglass|castle-keep/);
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
test('nightglass color schemes persist, validate, sync across tabs, and stay color-only',async()=>{
  const styles=new Map();
  const makeEnv=(savedMap=[])=>{
    const events=new Map(),links=[];
    const documentElement={
      dataset:{},
      style:{
        setProperty:(k,v)=>styles.set(k,v),
        removeProperty:k=>styles.delete(k),
      },
    };
    const makeLink=()=>({
      rel:'',href:'',dataset:{},sheet:null,blocking:'',
      setAttribute(){},
      addEventListener(type,fn){if(type==='load')queueMicrotask(()=>{this.sheet={};fn({});});},
      remove(){},
    });
    const document={
      readyState:'interactive',
      documentElement,
      head:{appendChild(node){links.push(node);}},
      createElement:()=>makeLink(),
      querySelector:()=>null,
      addEventListener(){},
    };
    const values=new Map(savedMap);
    const localStorage={
      get length(){return values.size;},
      key:i=>[...values.keys()][i]??null,
      getItem:k=>values.get(k)??null,
      setItem:(k,v)=>values.set(k,v),
      removeItem:k=>values.delete(k),
    };
    let changes=0;
    const window={addEventListener:(name,fn)=>events.set(name,fn),dispatchEvent:()=>{changes++;}};
    vm.runInNewContext(source,{Date:OrdinaryDate,window,document,localStorage,Event:class {}});
    return {
      themes:window.InfuserThemes,documentElement,values,
      storage:event=>events.get('storage')(event),
      get changes(){return changes;},
    };
  };

  const env=makeEnv();
  assert.equal(env.themes.current,'nightglass');
  assert.equal(env.themes.currentScheme,'default');
  assert.equal(env.documentElement.dataset.themeScheme,undefined);
  assert.ok(env.themes.schemesFor('nightglass').length>=12);
  assert.deepEqual([...env.themes.schemesFor('funny-pages')],[]);
  assert.deepEqual([...env.themes.schemesFor('cartridge-club')],[]);

  // Selecting a curated scheme updates dataset and persists cleanly.
  assert.equal(env.themes.selectScheme('twilight-violet'),'twilight-violet');
  assert.equal(env.themes.currentScheme,'twilight-violet');
  assert.equal(env.documentElement.dataset.themeScheme,'twilight-violet');
  assert.equal(styles.size,0);
  assert.equal(JSON.parse(env.values.get('infuser.theme.schemes')).nightglass,'twilight-violet');

  // Unknown schemes safely fall back to default.
  assert.equal(env.themes.selectScheme('not-a-scheme'),'default');
  assert.equal(env.themes.currentScheme,'default');
  assert.equal(env.documentElement.dataset.themeScheme,undefined);

  // Custom colors apply inline CSS tokens, sanitize bad values, and reset cleanly.
  const custom=env.themes.setCustomColors({primary:'#1a1025',accent:'#ff88cc',secondary:'not-a-color'});
  assert.deepEqual({...custom},{primary:'#1a1025',accent:'#ff88cc',secondary:'#e8d5a4'});
  assert.equal(env.themes.currentScheme,'custom');
  assert.equal(env.documentElement.dataset.themeScheme,'custom');
  assert.equal(styles.get('--panel'),'#1a1025');
  assert.equal(styles.get('--accent'),'#ff88cc');
  assert.equal(styles.get('--warm'),'#e8d5a4');

  // Switching to a theme without schemes clears data-theme-scheme and custom tokens.
  await env.themes.select('field-notes');
  assert.equal(env.themes.current,'field-notes');
  assert.equal(env.themes.currentScheme,'');
  assert.equal(env.documentElement.dataset.themeScheme,undefined);
  assert.equal(styles.size,0);

  // Switching back to Nightglass restores the saved custom scheme and tokens.
  await env.themes.select('nightglass');
  assert.equal(env.themes.currentScheme,'custom');
  assert.equal(env.documentElement.dataset.themeScheme,'custom');
  assert.equal(styles.get('--accent'),'#ff88cc');

  // Resetting custom colors restores defaults while staying in custom mode.
  const reset=env.themes.resetCustomColors();
  assert.deepEqual({...reset},{...env.themes.defaultCustom});
  assert.equal(styles.get('--accent'),env.themes.defaultCustom.accent);

  // Light, mid-gray, and white custom bases keep WCAG AA contrast on panels, surfaces, and inputs.
  const lum=hex=>{
    const n=parseInt(hex.slice(1),16);
    const [r,g,b]=[(n>>16)&255,(n>>8)&255,n&255].map(c=>{const s=c/255;return s<=0.03928?s/12.92:Math.pow((s+0.055)/1.055,2.4);});
    return 0.2126*r+0.7152*g+0.0722*b;
  };
  const ratio=(a,b)=>{const la=lum(a),lb=lum(b);return (Math.max(la,lb)+0.05)/(Math.min(la,lb)+0.05);};
  for(const primary of ['#ffffff','#f5efe6','#ffd700','#808080','#555555','#122128','#000000']){
    env.themes.setCustomColors({primary,accent:'#b5ead6',secondary:'#e8d5a4'});
    const ink=styles.get('--ink'),quiet=styles.get('--quiet');
    const panel=styles.get('--panel'),surface=styles.get('--surface'),field=styles.get('--field'),secBg=styles.get('--secondary-bg');
    assert.ok(ratio(ink,panel)>=4.5,`ink contrast on panel for ${primary}: ${ratio(ink,panel)}`);
    assert.ok(ratio(ink,surface)>=4.5,`ink contrast on surface for ${primary}: ${ratio(ink,surface)}`);
    assert.ok(ratio(ink,field)>=4.5,`ink contrast on field for ${primary}: ${ratio(ink,field)}`);
    assert.ok(ratio(ink,secBg)>=4.5,`ink contrast on secondary button for ${primary}: ${ratio(ink,secBg)}`);
    assert.ok(ratio(quiet,panel)>=4.0,`quiet contrast on panel for ${primary}: ${ratio(quiet,panel)}`);
    assert.ok(ratio(quiet,surface)>=4.0,`quiet contrast on surface for ${primary}: ${ratio(quiet,surface)}`);
  }
  assert.equal(styles.get('color-scheme'),'dark');
  env.themes.setCustomColors({primary:'#ffffff',accent:'#b5ead6',secondary:'#e8d5a4'});
  assert.equal(styles.get('color-scheme'),'light');

  // Cross-tab storage sync applies updated schemes.
  env.values.set('infuser.theme.schemes',JSON.stringify({nightglass:'deep-ocean'}));
  env.storage({key:'infuser.theme.schemes'});
  assert.equal(env.themes.currentScheme,'deep-ocean');
  assert.equal(env.documentElement.dataset.themeScheme,'deep-ocean');
  assert.equal(styles.size,0);

  // Every Nightglass scheme rule in nightglass.css is scoped and sets only CSS custom properties.
  const css=readFileSync(new URL('../web/themes/nightglass.css',import.meta.url),'utf8').replace(/\/\*[\s\S]*?\*\//g,'');
  const schemeBlocks=[...css.matchAll(/([^{}]*\[data-theme-scheme(?:="([^"]+)")?\][^{}]*)\{([^}]*)\}/g)];
  assert.ok(schemeBlocks.length>=13,'expected curated Nightglass scheme blocks');
  const defined=new Set();
  for(const [,selector,id,body] of schemeBlocks){
    assert.ok(selector.includes('[data-theme="nightglass"]'),selector);
    if(id)defined.add(id);
    for(const decl of body.split(';').map(s=>s.trim()).filter(Boolean)){
      assert.ok(decl.startsWith('--'),`Nightglass scheme "${id||'shared'}" must set color custom properties only, found: ${decl}`);
    }
  }
  for(const scheme of env.themes.schemesFor('nightglass')){
    if(scheme.id!=='default'&&scheme.id!=='custom')assert.ok(defined.has(scheme.id),`missing CSS block for ${scheme.id}`);
  }
  // Shared CSS keeps default Nightglass colors exact when no scheme is selected.
  const sharedCss=readFileSync(new URL('../web/clubhouse.css',import.meta.url),'utf8');
  assert.match(sharedCss,/\.text-button\s*\{[^}]*color:\s*var\(--text-button-ink,\s*#b4c9c9\)/);
  assert.match(sharedCss,/#notice\s*\{[^}]*color:\s*var\(--notice-ink,\s*#e0f3e8\)/);
  assert.match(sharedCss,/\.close\s*\{[^}]*color:\s*var\(--close-ink,\s*#b2c9c4\)/);
});
test('per-profile appearance caches by player_id, migrates once, and isolates cross-tab sync',async()=>{
  function makeTab({profile=null,values=new Map()}={}){
    const events=new Map(),styles=new Map(),links=[];
    const documentElement={
      dataset:{},
      style:{
        setProperty:(k,v)=>styles.set(k,v),
        removeProperty:k=>styles.delete(k),
      },
    };
    const makeLink=()=>({
      rel:'',href:'',dataset:{},sheet:null,blocking:'',
      setAttribute(){},
      addEventListener(type,fn){if(type==='load')queueMicrotask(()=>{this.sheet={};fn({});});},
      remove(){},
    });
    const document={
      readyState:'interactive',
      documentElement,
      head:{appendChild(node){links.push(node);}},
      createElement:()=>makeLink(),
      querySelector:selector=>{
        const id=/data-infuser-theme="([^"]+)"/.exec(selector);
        return id?links.find(link=>link.dataset.infuserTheme===id[1])||null:null;
      },
      addEventListener(){},
    };
    const localStorage={
      get length(){return values.size;},
      key:i=>[...values.keys()][i]??null,
      getItem:k=>values.get(k)??null,
      setItem:(k,v)=>values.set(k,v),
      removeItem:k=>values.delete(k),
    };
    const session=new Map(profile?[['infuser.profile',profile]]:[]);
    const sessionStorage={getItem:k=>session.get(k)??null,setItem:(k,v)=>session.set(k,v)};
    const window={addEventListener:(name,fn)=>events.set(name,fn),dispatchEvent:()=>{}};
    vm.runInNewContext(source,{Date:OrdinaryDate,window,document,localStorage,sessionStorage,Event:class {}});
    return {
      themes:window.InfuserThemes,documentElement,styles,values,
      storage:event=>events.get('storage')(event),
    };
  }

  // Legacy migration: an unset profile adopts a pre-deploy browser theme once.
  const shared=new Map([['infuser.theme','cartridge-club']]);
  const first=makeTab({profile:'pA',values:shared});
  assert.equal(first.themes.current,'cartridge-club','pre-paint uses legacy theme before bindProfile');
  const migrated=await first.themes.bindProfile('pA',null);
  assert.equal(migrated.theme,'cartridge-club');
  assert.equal(JSON.parse(shared.get('infuser.appearance.pA')).theme,'cartridge-club');

  // A profile with no saved look and no legacy keys stays exact default Nightglass (no migration needed).
  const clean=new Map();
  const fresh=makeTab({profile:'pClean',values:clean});
  const migratedClean=await fresh.themes.bindProfile('pClean',null);
  assert.equal(migratedClean,null);
  assert.equal(fresh.themes.current,'nightglass');
  assert.equal(fresh.documentElement.dataset.themeScheme,undefined);

  // Two open tabs on different profiles stay isolated; tabs on the same profile sync.
  const tabA1=makeTab({profile:'pA',values:clean});
  const tabA2=makeTab({profile:'pA',values:clean});
  const tabB=makeTab({profile:'pB',values:clean});
  await tabA1.themes.bindProfile('pA',{theme:'field-notes',schemes:{},custom:{}});
  await tabA2.themes.bindProfile('pA',{theme:'field-notes',schemes:{},custom:{}});
  await tabB.themes.bindProfile('pB',{theme:'nightglass',schemes:{nightglass:'amber-hearth'},custom:{}});
  assert.equal(tabA1.themes.current,'field-notes');
  assert.equal(tabB.themes.current,'nightglass');
  assert.equal(tabB.themes.currentScheme,'amber-hearth');

  // Tab A1 changes Profile A -> Tab A2 syncs, Tab B ignores it!
  await tabA1.themes.select('funny-pages');
  tabA2.storage({key:'infuser.appearance.pA'});
  tabB.storage({key:'infuser.appearance.pA'});
  tabB.storage({key:'infuser.theme.last'});
  await new Promise(r=>setTimeout(r,5));
  assert.equal(tabA2.themes.current,'funny-pages','same-profile tab syncs');
  assert.equal(tabB.themes.current,'nightglass','different-profile tab ignores other profile');
  assert.equal(tabB.themes.currentScheme,'amber-hearth');

  // Unpinned /profiles chooser tab uses browser's last theme (infuser.theme.last).
  const chooserTab=makeTab({profile:null,values:clean});
  assert.equal(chooserTab.themes.current,'funny-pages','chooser uses browser last theme');

  // Switching to a freshly cached profile paints its theme on boot without flashing last theme.
  chooserTab.themes.cacheProfile('pC',{theme:'nightglass',schemes:{nightglass:'deep-ocean'},custom:{}});
  const tabC=makeTab({profile:'pC',values:clean});
  assert.equal(tabC.themes.current,'nightglass');
  assert.equal(tabC.themes.currentScheme,'deep-ocean');

  // Pruning removes cached appearance entries for deleted profiles while keeping active ones and last theme.
  chooserTab.themes.pruneProfiles(['pA','pC']);
  assert.equal(clean.has('infuser.appearance.pA'),true);
  assert.equal(clean.has('infuser.appearance.pC'),true);
  assert.equal(clean.has('infuser.appearance.pB'),false,'deleted profile cache is removed');
  assert.equal(clean.has('infuser.theme.last'),true,'browser last theme stays intact');
});


test('Thanksgiving is only the fourth Thursday, with no saved holiday theme',async()=>{
  for(const [year,day] of [[2024,28],[2025,27],[2026,26],[2027,25],[2028,23],[2029,22],[2030,28]]){
    for(const offset of [-1,0,1,7]){
      const app=boot('castle-keep',{now:new Date(year,10,day+offset,12).getTime()});
      assert.equal(app.themes.seasonalLocked,offset===0,`${year} November ${day+offset}`);
      assert.equal(app.themes.current,offset===0?'thanksgiving':'castle-keep');
      assert.equal(app.themes.catalog.some(theme=>theme.id==='thanksgiving'),offset===0);
      assert.equal(app.themes.snapshot().theme,'castle-keep');
      assert.equal(app.values.get('infuser.theme'),'castle-keep');
      if(offset===0){await app.themes.select('nightglass');assert.equal(app.themes.current,'thanksgiving');}
    }
  }
});
test('holiday override restores custom colors and survives profile switches without saving it',async()=>{
  const app=boot('nightglass',{now:new Date(2026,10,25,23,59).getTime()});
  app.themes.setCustomColors({primary:'#123456',accent:'#fedcba',secondary:'#89abcd'});
  const saved=JSON.stringify(app.themes.snapshot()),storage=JSON.stringify([...app.values]);
  app.setDate(new Date(2026,10,26));const entering=app.themes.refreshSeason();app.load('thanksgiving');await entering;
  assert.equal(app.themes.current,'thanksgiving');assert.equal(JSON.stringify(app.themes.snapshot()),saved);
  app.themes.setCustomColors({primary:'#ffffff'});app.themes.resetCustomColors();app.themes.selectScheme('deep-ocean');await app.themes.select('funny-pages');
  assert.equal(JSON.stringify([...app.values]),storage,'holiday controls cannot overwrite preferences');
  app.setDate(new Date(2026,10,27));await app.themes.refreshSeason();
  assert.equal(app.themes.current,'nightglass');assert.equal(app.themes.currentScheme,'custom');assert.equal(JSON.stringify(app.themes.snapshot()),saved);
  assert.equal(JSON.stringify([...app.values]),storage,'entering and leaving only changes presentation');
  app.setDate(new Date(2026,10,26));await app.themes.refreshSeason();
  await app.themes.bindProfile('friend',{theme:'field-notes',schemes:{},custom:{}});
  assert.equal(app.themes.current,'thanksgiving');assert.equal(app.themes.snapshot().theme,'field-notes');
  app.setDate(new Date(2026,10,27));const leaving=app.themes.refreshSeason();app.load('field-notes');await leaving;
  assert.equal(app.themes.current,'field-notes');
});
