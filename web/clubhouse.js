const $ = id => document.getElementById(id);
const gameCount = $("game-count");
let tagHold, heldCard, suppressContextUntil=0;
let noticeTimer;
let me, library, currentReaction, refreshing = false, activeTag = "", editingTags, selectedTags = new Set();
let entryGame;
const entrySlug = /^\/games\/([a-z][a-z0-9-]{0,23})$/.exec(location.pathname)?.[1];
if (entrySlug) document.querySelector('.google').href='/oauth2/start?rd='+encodeURIComponent(location.pathname);
const emojis = [['⭐','Amazing'],['👍','Nice!'],['🚀','Exciting'],['❤️','Loved it'],['👏','Great job'],['💡','Cool idea']];
function el(tag, text, cls) { const node = document.createElement(tag); if(text !== undefined) node.textContent = text; if(cls) node.className = cls; return node; }
function notice(message) {
  clearTimeout(noticeTimer);
  const toast=$('notice');toast.textContent=message;toast.classList.add('visible');
  noticeTimer=setTimeout(()=>{toast.classList.remove('visible');toast.textContent='';},5000);
}
async function api(path, options = {}) {
  const r = await fetch(path, { ...options, headers: { 'Content-Type':'application/json', ...options.headers } });
  const result = await r.json();
  if(!r.ok) { const error = new Error(result.error || 'Please try again.'); error.status = r.status; throw error; }
  return result;
}
function showError(error) { notice(error.message); }
async function refresh() {
  if (refreshing) return; refreshing = true;
  try {
    library = await api('/api/library');
    if(entrySlug && !entryGame){entryGame=await api('/api/games/'+encodeURIComponent(entrySlug));history.replaceState(null,'',new URL(entryGame.url).pathname);document.title=entryGame.title+' · Infuser';}
    renderPresence();
    renderFilters(); renderGames(); renderFeed();
  } catch (error) { showError(error); } finally { refreshing = false; }
}
function renderGames() {
  const games = $('games');
  // Preserve ongoing launch controls and selections during presence polling.
  if (games.querySelector('[data-launching], .play-menu[open]')) return;
  cancelTagHold();
  games.replaceChildren();
  if(!library.apps.length) {
    const empty = el('div',undefined,'empty'); empty.append(el('h2','No games yet'),el('p','Add a game to get started.')); games.append(empty);
  }
  const query=$('game-search').value.trim().toLowerCase();
  const matching=library.apps.filter(g=>(!entrySlug||g.id===entryGame?.app_id)&&(!activeTag||(g.tags||[]).includes(activeTag))&&(!query||(g.title+' '+(g.tags||[]).join(' ')).toLowerCase().includes(query)));
  $('game-count').textContent=matching.length+' / '+library.apps.length+' games';
  if(library.apps.length&&!matching.length){const empty=el('div',undefined,'empty');empty.append(el('h2','No matches'),el('p','Try another tag or clear your search.'));games.append(empty);}
  for(const game of matching) {
    const card = el('article',undefined,'game');
    card.dataset.app=game.id;
    const art=el('div',undefined,'game-art game-mark');art.setAttribute('aria-hidden','true');
    if(game.card_art_url){const image=el('img');image.alt='';image.loading='lazy';image.decoding='async';image.onload=()=>art.classList.add('has-game-art');image.onerror=()=>image.remove();image.src=game.card_art_url;art.append(image);}
    const meta=el('div',undefined,'game-meta'),dock=el('div',undefined,'game-dock'),heading=el('div',undefined,'game-heading');
    const title=el('h2',game.title);title.title=game.title;heading.append(title);card.append(art,meta,dock);
    const modes = game.play_modes.map(m => m === 'solo' ? 'Single player' : 'Multiplayer').join(' + ');
    const mode=el('p',(game.preview ? 'ADMIN PREVIEW · ' : '') + modes,'game-mode');mode.title=mode.textContent;meta.append(mode);
    const tags=el('div',undefined,'game-tags');for(const tag of game.tags||[]){const chip=el('button',tagLabel(tag),'tag-chip');chip.title=tagLabel(tag);chip.onclick=()=>{activeTag=activeTag===tag?'':tag;renderFilters();renderGames();};tags.append(chip);}if(game.can_edit_tags){const edit=el('button','＋ Tags','edit-tags');edit.onclick=()=>openTags(game);tags.append(edit);}meta.append(tags);
    const names = [...new Set(game.players.map(p => p.name + (p.guest ? '’s guest' : '')))];
    const actions=el('div',undefined,'actions'), play=el('button','Play'), menu=el('details',undefined,'play-menu');
    const arrow=el('summary','▾');arrow.setAttribute('aria-label','Play options for '+game.title);arrow.title='Play options';
    const items=el('div',undefined,'play-menu-items');
    items.append(el('p',game.title,'menu-game-title'));
    const guest=el('button','Play in guest mode');guest.onclick=()=>{menu.open=false;start(game,play,card,{guest:true});};items.append(guest);
    if(!game.native_only && game.native_platforms?.length){const desktop=el('button','Play on computer');desktop.onclick=()=>{menu.open=false;startDesktop(game,play,card);};items.append(desktop);}
    for(const target of game.join_targets||[]){
      const join=el('button','Join '+target.name);join.onclick=()=>{menu.open=false;start(game,play,card,{intent:{kind:'join_friend',session_id:target.session_id,player_id:target.player_id}});};items.append(join);
    }
    if(game.url){const copy=el('button','Copy game link');copy.onclick=async()=>{try{await navigator.clipboard.writeText(game.url);menu.open=false;notice('Game link copied.');}catch{notice('Open '+game.url+' to share this game.');}};items.append(copy);}
    if(names.length)items.append(el('p',names.join(', ')+' · game open','players'));
    menu.append(arrow,items);menu.ontoggle=()=>card.classList.toggle('menu-open',menu.open);
    play.onclick=()=>start(game,play,card);actions.append(play,menu);
    const label=game.author?.name?'React to '+game.author.name+'’s game':'React to '+game.title;
    const react=el('button',undefined,'creator react-button');react.type='button';react.setAttribute('aria-label',label);react.setAttribute('aria-haspopup','dialog');
    const avatar=profileAvatar(game.author?.name,game.author?.id||game.id,game.author?.avatar),hint=el('span',label,'reaction-hint');hint.setAttribute('aria-hidden','true');
    react.append(avatar,hint);react.onclick=()=>openReaction(game);heading.append(react);dock.append(heading,actions);
    if(game.reactions.length){const chips=el('div',undefined,'chips');for(const [emoji]of emojis){const count=game.reactions.filter(r=>r.emoji===emoji).length;if(count)chips.append(el('span',emoji+' '+count,'chip'));}items.append(chips);}
    games.append(card);
  }
}
function profileAvatar(name,id,picture,size=128) {
  if((me?.avatars||[]).some(a=>a.id===picture)){
    const avatar=el('img',undefined,'avatar');avatar.src='/assets/avatars/'+encodeURIComponent(picture)+'/'+size+'.webp';avatar.alt='';avatar.width=size;avatar.height=size;return avatar;
  }
  const words=(name||'').trim().split(/\s+/).filter(Boolean);
  const initials=words.length>1?Array.from(words[0])[0]+Array.from(words.at(-1))[0]:Array.from(words[0]||'').slice(0,2).join('');
  const tone=Array.from(id||name||'').reduce((value,ch)=>(value+ch.codePointAt(0))%6,0);
  const avatar=el('span',initials.toLocaleUpperCase()||'···','avatar tone-'+tone);avatar.setAttribute('aria-hidden','true');return avatar;
}
function renderPresence() {
  const people=(library.friends||[]).filter(person=>person.id!==me.player_id),list=$('friends');list.replaceChildren();list.hidden=!people.length;
  for(const person of people.slice(0,2)){
    const item=el('li',undefined,'presence-person');item.tabIndex=0;item.setAttribute('aria-label',person.name+' · online');
    const name=el('span',person.name,'presence-name');name.setAttribute('aria-hidden','true');item.append(profileAvatar(person.name,person.id,person.avatar),name);list.append(item);
  }
  if(people.length>2){const extra=people.slice(2),item=el('li','+'+extra.length,'presence-person presence-more');item.tabIndex=0;item.setAttribute('aria-label',extra.map(p=>p.name).join(', ')+' · online');const names=el('span',extra.map(p=>p.name).join(' · '),'presence-name');names.setAttribute('aria-hidden','true');item.append(names);list.append(item);}
}
function cancelTagHold(){if(tagHold)clearTimeout(tagHold.timer);tagHold=null;}
$('games').addEventListener('focusin',event=>{
  if(event.target.matches('.game-tags > button'))event.target.scrollIntoView({block:'nearest',inline:'nearest'});
});
document.addEventListener('pointerdown',event=>{
  cancelTagHold();
  const card=event.target.closest('.game');
  if(event.pointerType!=='touch'||!event.isPrimary||!card||!$('games').contains(card)||event.target.closest('button,select,input,textarea,a,summary,details'))return;
  const game=library?.apps.find(g=>g.id===card.dataset.app);if(!game?.can_edit_tags)return;
  tagHold={pointer:event.pointerId,x:event.clientX,y:event.clientY,card,timer:setTimeout(()=>{
    if(!card.isConnected){cancelTagHold();return;}
    heldCard=card;suppressContextUntil=Date.now()+1200;
    cancelTagHold();openTags(game);
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
async function start(game,button,card,{guest=false,intent}={}) {
  if(game.native_only)return startDesktop(game,button,card,{guest});
  // Open synchronously in the click event so browser popup blockers allow Play.
  const tab = window.open('about:blank','_blank');
  if(!tab){notice('Allow this site to open a game tab, then press Play again.');return;}
  tab.document.title='Opening '+game.title;tab.document.body.textContent='Getting '+game.title+' ready…';
  card.dataset.launching='true';button.disabled=true;button.textContent='Starting…';
  try {
    const launch=await api(`/api/apps/${game.id}/launch`,{method:'POST',body:JSON.stringify({guest,intent})});
    const deadline=Date.now()+120000;let state=launch.state;
    while(state!=='ready') {
      if(tab.closed)throw Error('Game launch cancelled.');
      if(Date.now()>deadline)throw Error('This game is taking a while. Try Play again shortly.');
      await new Promise(resolve=>setTimeout(resolve,1500));
      const status=await api('/api/launches/'+launch.launch_id);state=status.state;
      if(state==='failed'||state==='stopping')throw Error(status.error||'The game could not start. Its creator can try an update.');
    }
    tab.opener=null;tab.location.replace(launch.url);
  } catch(error) {tab.close();showError(error);} finally {delete card.dataset.launching;button.disabled=false;button.textContent='Play';}
}
function desktopPlatform(){
  const platform=navigator.userAgentData?.platform||navigator.platform;
  if(/Win/i.test(platform))return 'windows-x86_64';
  if(/Linux/i.test(platform)&&!/Android/i.test(navigator.userAgent))return 'linux-x86_64';
  return null;
}
async function startDesktop(game,button,card,{guest=false}={}){
  const platform=desktopPlatform();
  if(!game.native_platforms?.includes(platform)){notice('This desktop game needs a supported Windows or Linux computer.');return;}
  card.dataset.launching='true';button.disabled=true;button.textContent='Starting…';
  try{
    const launch=await api(`/api/native/v1/play/${game.id}`,{method:'POST',body:JSON.stringify({platform,guest})});
    document.querySelector('#desktop-launch')?.remove();
    const dialog=el('dialog');dialog.id='desktop-launch';dialog.setAttribute('aria-label','Open '+game.title);
    const heading=el('h2','Let’s play'),text=el('p','Open Infuser when your browser asks. First time? Install it, then press Play again.');
    const open=el('a','Open Infuser','button');open.href=launch.url;
    const install=el('a','Install Infuser','button secondary');
    install.href='https://github.com/esper256/infuser/releases/latest/download/'+(platform==='windows-x86_64'?'Infuser-Setup.exe':'Infuser.flatpak');
    const close=el('button','Done','secondary');close.onclick=()=>dialog.close();
    const actions=el('div',undefined,'actions');actions.append(open,install,close);dialog.append(heading,text,actions);document.body.append(dialog);dialog.showModal();
    // Keep a real link available if the browser requires a fresh user gesture.
    open.click();
  }catch(error){showError(error);}finally{delete card.dataset.launching;button.disabled=false;button.textContent='Play';}
}
document.addEventListener('click',event=>{for(const menu of document.querySelectorAll('.play-menu[open]'))if(!menu.contains(event.target))menu.open=false;});
document.addEventListener('keydown',event=>{if(event.key==='Escape')for(const menu of document.querySelectorAll('.play-menu[open]')){menu.open=false;menu.querySelector('summary').focus();}});
function renderFeed() {
  const feed=$('feed');feed.replaceChildren();const entries=[];
  for(const game of library.apps){entries.push({at:game.updated,emoji:'↗',title:game.title+' updated',text:'A new build is ready to play.'});for(const r of game.reactions)entries.push({at:r.updated,emoji:r.emoji,title:r.name+' reacted to '+game.title,text:r.text});}
  entries.sort((a,b)=>b.at-a.at);
  if(!entries.length)feed.append(el('p','Game updates and reactions show up here.','quiet'));
  for(const entry of entries.slice(0,12)){const row=el('div',undefined,'feed-row'),body=el('div');body.append(el('small',entry.title));if(entry.text)body.append(el('p',entry.text));row.append(el('span',entry.emoji,'feed-emoji'),body);feed.append(row);}
}
for(const [emoji,label]of emojis){const l=el('label'),radio=el('input');radio.type='radio';radio.name='emoji';radio.value=emoji;radio.required=true;radio.setAttribute('aria-label',label);l.append(radio,el('span',emoji));$('emoji').append(l);}
function openReaction(game){currentReaction=game;$('reaction-comment').hidden=true;$('reaction-title').textContent=game.title;const mine=game.reactions.find(r=>r.mine);$('reaction-form').reset();if(mine){$('reaction-comment').hidden=false;$('reaction-form').elements.emoji.value=mine.emoji;$('reaction-form').elements.text.value=mine.text;}$('remove-reaction').hidden=!mine;$('reaction-dialog').showModal();}
$('close-reaction').onclick=()=>$('reaction-dialog').close();
$('reaction-form').onsubmit=async event=>{event.preventDefault();try{const f=new FormData(event.target);await api(`/api/apps/${currentReaction.id}/reaction`,{method:'PUT',body:JSON.stringify({emoji:f.get('emoji'),text:f.get('text')})});$('reaction-dialog').close();celebrate(document.querySelector('.games'),f.get('emoji'));notice('Reaction sent!');await refresh();}catch(error){showError(error);}};
$('remove-reaction').onclick=async()=>{try{await api(`/api/apps/${currentReaction.id}/reaction`,{method:'DELETE'});$('reaction-dialog').close();await refresh();}catch(error){showError(error);}};
async function adminRefresh(){
  const [members,credentials]=await Promise.all([api('/api/admin/members'),api('/api/admin/credentials')]);
  $('members').replaceChildren();for(const[email,m]of Object.entries(members.members)){const row=el('div');row.append(el('span',`${m.name} · ${email}${m.admin?' · admin':''}${m.enabled?'':' · disabled'}`));if(!m.admin){const toggle=el('button',m.enabled?'Disable':'Enable','secondary');toggle.onclick=async()=>{try{await api('/api/admin/members',{method:'POST',body:JSON.stringify({email,name:m.name,admin:m.admin,enabled:!m.enabled})});await adminRefresh();}catch(error){showError(error);}};row.append(toggle);}$('members').append(row);}
  $('credentials').replaceChildren();for(const c of credentials.credentials){const row=el('div'),button=el('button','Revoke','secondary');row.append(el('span',c.label+' · '+c.app),button);button.onclick=async()=>{try{await api('/api/admin/credentials/'+c.id,{method:'DELETE'});await adminRefresh();}catch(error){showError(error);}};$('credentials').append(row);}
}
$('invite-form').onsubmit=async event=>{event.preventDefault();const data=Object.fromEntries(new FormData(event.target));try{await api('/api/admin/members',{method:'POST',body:JSON.stringify(data)});event.target.reset();notice('Invitation saved. They can sign in with that Google account.');await adminRefresh();}catch(error){showError(error);}};
$('credential-form').onsubmit=async event=>{event.preventDefault();try{const data=Object.fromEntries(new FormData(event.target)),connection=await api('/api/admin/credentials',{method:'POST',body:JSON.stringify(data)});const blob=new Blob([JSON.stringify(connection,null,2)+'\n'],{type:'application/json'}),url=URL.createObjectURL(blob),link=el('a');link.href=url;link.download=connection.app_id+'.credentials.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);notice('Connection file downloaded. Attach it in your game’s existing agent chat.');await adminRefresh();}catch(error){showError(error);}};
function renderAccount(){
  $('open-profile').replaceChildren(profileAvatar(me.name,me.player_id,me.avatar),el('span',me.name));
}
function openProfile(){
  $('profile-error').textContent='';
  const picker=$('profile-options');picker.replaceChildren();
  const selected=(me.avatars||[]).some(a=>a.id===me.avatar)?me.avatar:'';
  for(const option of [{id:'',name:'Initials'},...(me.avatars||[])]){
    const label=el('label',undefined,'profile-option'),radio=el('input'),tile=el('span',undefined,'profile-tile');
    radio.type='radio';radio.name='avatar';radio.value=option.id;radio.checked=option.id===selected;radio.required=true;
    tile.append(profileAvatar(me.name,me.player_id,option.id,256),el('span',option.name,'profile-caption'));
    label.append(radio,tile);picker.append(label);
  }
  $('profile-dialog').showModal();picker.querySelector('input:checked').focus();
}
$('close-profile').onclick=()=>$('profile-dialog').close();
$('profile-dialog').addEventListener('cancel',event=>{if($('save-profile').disabled)event.preventDefault();});
$('profile-form').onsubmit=async event=>{
  event.preventDefault();if($('save-profile').disabled)return;
  const avatar=new FormData(event.target).get('avatar');
  $('profile-error').textContent='';$('save-profile').disabled=true;$('save-profile').textContent='Saving…';$('close-profile').disabled=true;$('profile-choices').disabled=true;
  try{
    const saved=await api('/api/profile',{method:'PUT',body:JSON.stringify({avatar})});
    me.avatar=saved.avatar;renderAccount();
    if(library){for(const game of library.apps)if(game.author?.id===me.player_id)game.author.avatar=me.avatar;for(const person of library.friends||[])if(person.id===me.player_id)person.avatar=me.avatar;renderGames();renderPresence();}
    $('profile-dialog').close();notice('Profile picture saved.');
  }catch(error){$('profile-error').textContent=error.message;}
  finally{$('save-profile').disabled=false;$('save-profile').textContent='Save';$('close-profile').disabled=false;$('profile-choices').disabled=false;}
};

try {
  me=await api('/api/me');$('welcome').hidden=true;$('clubhouse').hidden=false;
  $('greeting').textContent='Signed in as '+me.name+'.';const profile=el('button',undefined,'profile-button');profile.id='open-profile';profile.type='button';profile.setAttribute('aria-haspopup','dialog');profile.setAttribute('aria-label',me.name+' · Change profile picture');profile.onclick=openProfile;$('account').append(profile);renderAccount();const out=el('a','Sign out');out.href='/oauth2/sign_out?rd=%2F';$('account').append(out);
  await refresh();setInterval(()=>{if(!document.hidden)refresh();},25000);
  if(me.admin){$('admin').hidden=false;await adminRefresh();}
} catch(error) {if(error.status!==401)$('login-message').textContent=error.message;}

let newGameConnection, setupPending=false;
function showSetupPrompt() {
  const engines=(newGameConnection.supported_engines||[{label:'Three.js / Vite'},{label:'plain HTML/JavaScript'}]).map(engine=>engine.label);
  $('engine-support').textContent='Supported now: '+engines.join(' and ')+'. More engines are coming.';
  $('agent-prompt').value=`Publish the game in this existing project to Infuser. Continue this game; do not replace it or start a new project.

Read https://github.com/esper256/infuser/blob/main/docs/agents.md and use the recipe for the engine already in this project. Preserve the gameplay, art, and project structure. This game already exists in this project. Detect its current title and engine from the project; use that title in infuser.game.json. Do not ask me to rename it or rebuild it from scratch.

ONE-TIME SETUP CODE (expires after one hour):
${newGameConnection.setup_code}

First inspect this project for an existing Infuser integration and working credentials for this same game on ${newGameConnection.api_url}. If present, reuse that connection and its original app ID; ignore the provided setup code and update the existing game instead of creating a duplicate.

Otherwise, use the reserved app ID ${newGameConnection.app_id}. Get the current Infuser CLI from the repository's release instructions. Feed the setup code to 'infuser connect --server ${newGameConnection.api_url} --app ${newGameConnection.app_id}' on stdin. It exchanges the code once and saves the permanent game-scoped credentials under .local/. Keep .local/ and *.credentials.json out of Git and game exports. Do not print or echo setup tokens or permanent credentials. If this project already has working credentials for this app ID, reuse them.

Available tag IDs in this Infuser: ${(newGameConnection.tag_catalog||[]).map(t=>t.id).join(', ')}. Choose a few fitting default tags from the game’s design and code; prefer these existing IDs. Put them in infuser.game.json. Query the current tag catalog after connecting if needed.

Inspect the engine and build scripts, declare the game's actual Single player / Multiplayer capabilities, integrate cloud saves and isolated guest play, build, publish, and verify the result. Handle routine setup yourself. Do not ask me to configure Docker, DNS, ports, or hosting. If the existing multiplayer transport is unsupported, explain the exact blocker instead of downgrading it.

Future updates must reuse the saved connection and the same app ID. Tell me when the game is ready to play in my library.`;
  $('add-game-title').textContent='Bring your game';
  $('refresh-setup').textContent='Refresh code';
  $('setup-expiry').textContent='One use · expires '+new Date(newGameConnection.expires_at*1000).toLocaleTimeString([],{hour:'numeric',minute:'2-digit'})+'.';
  $('agent-kit').hidden=false;$('copy-prompt').disabled=false;$('copy-prompt').textContent='Copy prompt';
}
async function prepareGame() {
  if(setupPending)return;setupPending=true;
  $('agent-kit').hidden=false;$('agent-prompt').value='Getting your prompt ready…';$('setup-expiry').textContent='';
  $('copy-prompt').disabled=true;$('refresh-setup').disabled=true;$('another-game').disabled=true;
  try { newGameConnection=await api('/api/my-games',{method:'POST',body:'{}'});showSetupPrompt(); }
  catch(error){$('agent-prompt').value=error.message;$('refresh-setup').textContent='Try again';}
  finally {setupPending=false;$('refresh-setup').disabled=false;$('another-game').disabled=false;}
}
$('add-game').onclick=()=>{$('add-game-dialog').showModal();if(newGameConnection)showSetupPrompt();else prepareGame();};
$('close-add-game').onclick=()=>$('add-game-dialog').close();
async function refreshSetup() {
  if(setupPending)return false;
  if(!newGameConnection){await prepareGame();return !!newGameConnection;}
  setupPending=true;$('copy-prompt').disabled=true;$('refresh-setup').disabled=true;$('another-game').disabled=true;
  try {newGameConnection=await api('/api/my-games/'+newGameConnection.app_id+'/setup',{method:'POST'});showSetupPrompt();return true;}
  catch(error){$('setup-expiry').textContent=error.message;return false;}
  finally {setupPending=false;$('copy-prompt').disabled=false;$('refresh-setup').disabled=false;$('another-game').disabled=false;}
}
$('copy-prompt').onclick=async()=>{
  if(!newGameConnection||setupPending)return;
  if(Date.now()>=newGameConnection.expires_at*1000 && !await refreshSetup())return;
  try {await navigator.clipboard.writeText($('agent-prompt').value);$('copy-prompt').textContent='Copied — paste it into your game’s chat';}
  catch {$('agent-prompt').focus();$('agent-prompt').select();$('copy-prompt').textContent='Select and copy the prompt above';}
};
$('refresh-setup').onclick=refreshSetup;
$('another-game').onclick=()=>{if(setupPending)return;newGameConnection=null;prepareGame();};
function tagLabel(id){return library?.tags?.find(t=>t.id===id)?.label||id.split('-').map(w=>w[0]?.toUpperCase()+w.slice(1)).join(' ');}
function renderFilters(){const nav=$('tag-filters');nav.replaceChildren();const all=el('button','All games','tag-filter'+(!activeTag?' selected':''));all.setAttribute('aria-pressed',String(!activeTag));all.onclick=()=>{activeTag='';renderFilters();renderGames();};nav.append(all);for(const tag of library.tags||[]){if(!tag.count&&activeTag!==tag.id)continue;const button=el('button',tag.label+' '+tag.count,'tag-filter'+(activeTag===tag.id?' selected':''));button.setAttribute('aria-pressed',String(activeTag===tag.id));button.onclick=()=>{activeTag=activeTag===tag.id?'':tag.id;renderFilters();renderGames();};nav.append(button);}nav.append(gameCount);}
$('game-search').oninput=()=>{if(library)renderGames();};
function openTags(game){editingTags=game;selectedTags=new Set(game.tags||[]);$('tags-title').textContent='Tags: '+game.title;renderTagPicker();$('tags-dialog').showModal();}
function renderTagPicker(){const picker=$('tag-picker');picker.replaceChildren();const ids=new Set([...(library.tags||[]).map(t=>t.id),...selectedTags]);for(const id of [...ids].sort()){const button=el('button',tagLabel(id),'tag-chip'+(selectedTags.has(id)?' selected':''));button.type='button';button.setAttribute('aria-pressed',String(selectedTags.has(id)));button.onclick=()=>{if(selectedTags.has(id))selectedTags.delete(id);else if(selectedTags.size<8)selectedTags.add(id);renderTagPicker();};picker.append(button);}$('tag-selection-count').textContent=selectedTags.size+' / 8 selected';}
$('close-tags').onclick=()=>$('tags-dialog').close();
$('new-tag-form').onsubmit=event=>{event.preventDefault();const id=$('new-tag').value.trim().toLowerCase().replace(/\s+/g,'-');if(!/^[a-z0-9](?:[a-z0-9-]{0,30}[a-z0-9])?$/.test(id)){$('new-tag').setCustomValidity('Use letters, numbers and dashes, up to 32 characters.');$('new-tag').reportValidity();return;}$('new-tag').setCustomValidity('');if(selectedTags.size<8){selectedTags.add(id);$('new-tag').value='';renderTagPicker();}};
$('new-tag').oninput=()=>$('new-tag').setCustomValidity('');
$('save-tags').onclick=async()=>{try{await api('/api/apps/'+editingTags.id+'/tags',{method:'PUT',body:JSON.stringify({tags:[...selectedTags]})});$('tags-dialog').close();notice('Tags updated.');await refresh();}catch(error){showError(error);}};
$('emoji').addEventListener('change',()=>{$('reaction-comment').hidden=false;$('reaction-text').focus();const selected=$('emoji').querySelector('input:checked+span');if(selected&&!matchMedia('(prefers-reduced-motion: reduce)').matches)selected.animate([{transform:'scale(.85) rotate(-8deg)'},{transform:'scale(1.17) rotate(5deg)'},{transform:'scale(1) rotate(0)'}],{duration:260,easing:'ease-out'});});
$('reaction-text').addEventListener('input',()=>{const selected=$('emoji').querySelector('input:checked+span');if(selected&&!matchMedia('(prefers-reduced-motion: reduce)').matches)selected.animate([{transform:'translateY(0)'},{transform:'translateY(-3px)'},{transform:'translateY(0)'}],{duration:130});});
function celebrate(target,emoji='✦'){if(!target||matchMedia('(prefers-reduced-motion: reduce)').matches)return;const bounds=target.getBoundingClientRect();const burst=el('span',emoji,'mini-celebration');burst.style.left=Math.min(innerWidth-50,Math.max(24,bounds.left+bounds.width/2))+'px';burst.style.top=Math.min(innerHeight-80,Math.max(80,bounds.top+50))+'px';document.body.append(burst);burst.addEventListener('animationend',()=>burst.remove(),{once:true});}
async function refreshNetworkHealth(){
  if(!me?.admin||!$('admin').querySelector('details')?.open)return;
  let box=$('network-health');if(!box){box=el('section');box.id='network-health';$('admin').querySelector('.admin-panel').append(box);}
  try{
    const data=await api('/api/admin/peer-health');box.replaceChildren(el('h3','Network diagnostics'));
    if(!data.sessions.length){box.append(el('p','No active peer sessions.','quiet'));return;}
    const milliseconds=value=>typeof value==='number'&&Number.isFinite(value)?Math.round(value)+' ms':'measuring';
    for(const session of data.sessions){
      box.append(el('h4',session.app_id));
      for(const peer of session.peers){
        const connections=peer.health?.connections||[];const age=Date.now()/1000-peer.reported_at;
        const prefix=peer.name+(peer.host?' · host':' · player');
        if(!connections.length)box.append(el('p',prefix+' · waiting for a connection','quiet'));
        for(const link of connections)box.append(el('p',prefix+' · '+(link.local||'?')+' ↔ '+(link.remote||'?')+' · round trip '+milliseconds(link.round_trip_ms)+(age>15?' · stale report':'')+' · '+Math.round((Number(link.bytesSent)||0)/1024)+' KiB sent / '+Math.round((Number(link.bytesReceived)||0)/1024)+' KiB received'));
      }
    }
    box.append(el('p','Browser-reported network round trips. Rendering and game simulation can add delay.','quiet'));
  }catch{box.replaceChildren(el('h3','Network diagnostics'),el('p','Diagnostics unavailable. Games can keep running.','quiet'));}
}
$('admin').querySelector('details').addEventListener('toggle',refreshNetworkHealth);
setInterval(refreshNetworkHealth,5000);
