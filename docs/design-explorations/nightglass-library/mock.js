// Fictional fixtures for a standalone visual review. No API requests or real launches.
const $=id=>document.getElementById(id);
const node=(tag,text,cls)=>{const e=document.createElement(tag);if(text!==undefined)e.textContent=text;if(cls)e.className=cls;return e;};
const games=[
  ['Firefly Drift','KwamiKwami','Chill',-1,8],['Moon Racers','OrbitFox','Racing',0,0],['Mossbyte','ByteMoth','Adventure',1,1],
  ['Tide Runner','OrbitFox','Racing',2,0],['Star Courier','KwamiKwami','Arcade',3,8],['Ghost Circuit','ByteMoth','Racing',4,1],
  ['Cloudforge','OrbitFox','Adventure',5,0],['Ember Rail','KwamiKwami','Arcade',6,8],['Deep Signal','ByteMoth','Chill',7,1]
].map(([title,author,tag,art,avatar],id)=>({id,title,author,tags:[tag],art,avatar,guest:'player'}));
let category='',editing,current;
const notify=text=>{$('notice').textContent=text;$('notice').classList.add('visible');};
function render(){
  cancelTagHold();
  const q=$('game-search').value.toLowerCase(),visible=games.filter(g=>(!category||g.tags.includes(category))&&g.title.toLowerCase().includes(q));
  $('games').replaceChildren();$('game-count').textContent=visible.length+' / '+games.length+' games';
  for(const g of visible){
    const card=node('article',undefined,'game');card.dataset.game=g.id;
    const art=node('div',undefined,'game-art'+(g.art===-1?' firefly':''));art.setAttribute('aria-hidden','true');art.style.setProperty('--art-x',(g.art%3)*50+'%');art.style.setProperty('--art-y',Math.floor(g.art/3)*50+'%');card.append(art);
    const meta=node('div',undefined,'game-meta'),tags=node('div',undefined,'game-tags');
    for(const label of g.tags){const t=node('button',label,'tag-chip');t.onclick=()=>{category=category===label?'':label;renderFilters();render();};tags.append(t);}
    const edit=node('button','＋ Tags','edit-tags');edit.setAttribute('aria-label','Edit tags for '+g.title);edit.onclick=()=>openTags(g);tags.append(edit);meta.append(tags);card.append(meta);
    const dock=node('div',undefined,'game-dock'),heading=node('div',undefined,'game-heading');heading.append(node('h2',g.title));
    const creator=node('button',undefined,'creator');creator.setAttribute('aria-label','React to '+g.author+'’s game');creator.setAttribute('aria-haspopup','dialog');
    const avatar=node('span',undefined,'avatar');avatar.setAttribute('aria-hidden','true');avatar.style.setProperty('--avatar-x',g.avatar%3*50+'%');avatar.style.setProperty('--avatar-y',Math.floor(g.avatar/3)*50+'%');
    const hint=node('span','React to '+g.author+'’s game','reaction-hint');hint.setAttribute('aria-hidden','true');creator.append(avatar,hint);creator.onclick=()=>{current=g;$('reaction-title').textContent=g.title;$('reaction-form').reset();$('reaction-comment').hidden=true;$('remove-reaction').hidden=true;$('reaction-dialog').showModal();};heading.append(creator);
    const actions=node('div',undefined,'actions'),play=node('button','Play'),select=node('select');select.setAttribute('aria-label','Play options for '+g.title);
    for(const [value,label]of [['player','My progress'],['guest','Play in guest mode']]){const option=node('option',label);option.value=value;select.append(option);}select.value=g.guest;select.onchange=()=>{g.guest=select.value;};play.onclick=()=>notify('Preview only · '+g.title+(g.guest==='guest'?' · guest mode':''));actions.append(play,select);dock.append(heading,actions);card.append(dock);$('games').append(card);
  }
  if(!visible.length){const empty=node('div',undefined,'empty');empty.append(node('h2','No matches'),node('p','Try another tag or search.'));$('games').append(empty);}
}
// Touch-only hold on the card surface; controls keep their own gestures.
let tagHold, heldCard, suppressContextUntil=0;
function cancelTagHold(){if(tagHold)clearTimeout(tagHold.timer);tagHold=null;}
document.addEventListener('pointerdown',event=>{
  cancelTagHold();
  const card=event.target.closest('.game');
  if(event.pointerType!=='touch'||!event.isPrimary||!card||!$('games').contains(card)||event.target.closest('button,select,input,textarea,a,summary'))return;
  tagHold={pointer:event.pointerId,x:event.clientX,y:event.clientY,card,timer:setTimeout(()=>{
    if(!card.isConnected){cancelTagHold();return;}
    heldCard=card;suppressContextUntil=Date.now()+1200;
    cancelTagHold();openTags(games.find(g=>String(g.id)===card.dataset.game));
  },600)};
},{passive:true});
document.addEventListener('pointermove',event=>{
  if(tagHold&&event.pointerId===tagHold.pointer&&Math.hypot(event.clientX-tagHold.x,event.clientY-tagHold.y)>10)cancelTagHold();
},{passive:true});
for(const type of ['pointerup','pointercancel'])document.addEventListener(type,cancelTagHold,{passive:true});
window.addEventListener('scroll',cancelTagHold,{capture:true,passive:true});
window.addEventListener('blur',cancelTagHold);
document.addEventListener('visibilitychange',()=>{if(document.hidden)cancelTagHold();});
$('games').addEventListener('contextmenu',event=>{
  if(tagHold||Date.now()<suppressContextUntil&&heldCard?.contains(event.target))event.preventDefault();
});
function renderFilters(){const nav=$('tag-filters');nav.replaceChildren();for(const label of ['',...new Set(games.flatMap(g=>g.tags))]){const button=node('button',label||'All games','tag-filter'+(category===label?' selected':''));button.setAttribute('aria-pressed',String(category===label));button.onclick=()=>{category=label;renderFilters();render();};nav.append(button);}}
function openTags(game){editing=game;$('tags-title').textContent='Tags: '+game.title;$('tag-picker').replaceChildren();for(const label of ['Arcade','Adventure','Racing','Chill']){const b=node('button',label,'tag-chip'+(game.tags.includes(label)?' selected':''));b.setAttribute('aria-pressed',String(game.tags.includes(label)));b.onclick=()=>{b.classList.toggle('selected');b.setAttribute('aria-pressed',String(b.classList.contains('selected')));};$('tag-picker').append(b);}$('tag-selection-count').textContent='Preview';$('tags-dialog').showModal();}
$('save-tags').onclick=()=>{editing.tags=[...$('tag-picker').querySelectorAll('.selected')].map(e=>e.textContent);$('tags-dialog').close();renderFilters();render();};
$('new-tag-form').onsubmit=e=>{e.preventDefault();const text=$('new-tag').value.trim();if(!text)return;const b=node('button',text,'tag-chip selected');b.setAttribute('aria-pressed','true');b.onclick=()=>{b.classList.toggle('selected');b.setAttribute('aria-pressed',String(b.classList.contains('selected')));};$('tag-picker').append(b);$('new-tag').value='';};
for(const [emoji,label]of [['⭐','Amazing'],['👍','Nice'],['🚀','Exciting'],['❤️','Loved it'],['👏','Great job'],['💡','Cool idea']]){const l=node('label'),input=node('input');input.type='radio';input.name='emoji';input.value=emoji;input.required=true;input.setAttribute('aria-label',label);l.append(input,node('span',emoji));$('emoji').append(l);}
$('emoji').onchange=()=>{$('reaction-comment').hidden=false;$('reaction-text').focus();};
$('reaction-form').onsubmit=e=>{e.preventDefault();$('reaction-dialog').close();notify('Preview reaction to '+current.author+'’s game');};
for(const name of ['reaction','tags','add-game'])$('close-'+name).onclick=()=>$(name==='reaction'?'reaction-dialog':name+'-dialog').close();
$('add-game').onclick=()=>{$('add-game-dialog').showModal();};
$('add-game-form').onsubmit=e=>{e.preventDefault();$('add-game-title').textContent='Ready for your agent';$('add-game-form').hidden=true;$('agent-kit').hidden=false;$('agent-prompt').value='Preview only: publish “'+new FormData(e.target).get('title')+'” to Infuser. No setup code is generated in this mockup.';$('setup-expiry').textContent='Preview · no credentials';};
$('copy-prompt').onclick=async()=>{try{await navigator.clipboard.writeText($('agent-prompt').value);$('copy-prompt').textContent='Copied';}catch{$('agent-prompt').select();}};
$('another-game').onclick=()=>{$('add-game-title').textContent='Add a game';$('agent-kit').hidden=true;$('add-game-form').hidden=false;};$('refresh-setup').onclick=()=>notify('Preview · no setup code generated');
$('game-search').oninput=render;
$('friends').textContent='OrbitFox · ByteMoth';$('account').append(node('span','KwamiKwami'));const out=node('a','Sign out');out.href='#';out.onclick=e=>{e.preventDefault();notify('Preview only');};$('account').append(out);$('admin').hidden=false;
for(const id of ['invite-form','credential-form'])$(id).onsubmit=e=>{e.preventDefault();notify('Preview only · nothing saved');};
for(const [emoji,text]of [['🚀','OrbitFox reacted to Firefly Drift'],['↗','Mossbyte updated'],['👏','KwamiKwami reacted to Tide Runner']]){const row=node('div',undefined,'feed-row');row.append(node('span',emoji,'feed-emoji'),node('small',text));$('feed').append(row);}
renderFilters();render();
