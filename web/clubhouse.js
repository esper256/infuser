const $ = id => document.getElementById(id);
let me, library, currentReaction, refreshing = false, activeTag = "", editingTags, selectedTags = new Set();
const emojis = [['⭐','Amazing'],['👍','Nice!'],['🚀','Exciting'],['❤️','Loved it'],['👏','Great job'],['💡','Cool idea']];
function el(tag, text, cls) { const node = document.createElement(tag); if(text !== undefined) node.textContent = text; if(cls) node.className = cls; return node; }
function notice(message) { $('notice').textContent = message; $('notice').classList.add('visible'); }
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
    $('friends').textContent = library.friends.map(f => f.name).join(' · ') || 'No one else is online yet.';
    renderFilters(); renderGames(); renderFeed();
  } catch (error) { showError(error); } finally { refreshing = false; }
}
function renderGames() {
  const games = $('games');
  // Preserve ongoing launch controls and selections during presence polling.
  if (games.querySelector('[data-launching]')) return;
  const choices = new Map([...games.querySelectorAll('select')].map(s => [s.dataset.app,s.value]));
  games.replaceChildren();
  if(!library.apps.length) {
    const empty = el('div',undefined,'empty'); empty.append(el('h2','Your library starts here.'),el('p',me.admin ? 'Hit Add a game, then give your existing game-making agent the setup prompt. Your published game will appear here.' : 'Already making a game? Hit Add a game and bring it over.')); games.append(empty);
  }
  const query=$('game-search').value.trim().toLowerCase();
  const matching=library.apps.filter(g=>(!activeTag||(g.tags||[]).includes(activeTag))&&(!query||(g.title+' '+(g.tags||[]).join(' ')).toLowerCase().includes(query)));
  $('game-count').textContent=matching.length+' / '+library.apps.length+' games';
  if(library.apps.length&&!matching.length){const empty=el('div',undefined,'empty');empty.append(el('h2','No matches yet.'),el('p','Try another tag or clear your search.'));games.append(empty);}
  for(const game of matching) {
    const card = el('article',undefined,'game');
    card.append(el('div',game.play_modes.includes('multiplayer')?'✦':'◈','game-mark'),el('h2',game.title));
    const modes = game.play_modes.map(m => m === 'solo' ? 'Single player' : 'Multiplayer').join(' + ');
    card.append(el('p',(game.preview ? 'ADMIN PREVIEW · ' : '') + modes,'quiet'));
    const tags=el('div',undefined,'game-tags');for(const tag of game.tags||[]){const chip=el('button',tagLabel(tag),'tag-chip');chip.onclick=()=>{activeTag=activeTag===tag?'':tag;renderFilters();renderGames();};tags.append(chip);}if(game.can_edit_tags){const edit=el('button','＋ Tags','edit-tags');edit.onclick=()=>openTags(game);tags.append(edit);}card.append(tags);
    const names = [...new Set(game.players.map(p => p.name + (p.guest ? '’s guest' : '')))];
    card.append(el('p',names.length ? names.join(', ') + ' · game open' : 'Ready to launch','players'));
    const actions=el('div',undefined,'actions'), play=el('button','Play'), select=el('select'); select.dataset.app=game.id;select.setAttribute('aria-label','Play options for '+game.title);
    for(const guest of [false,true]) {const option=el('option',guest ? 'Play in guest mode' : 'My progress');option.value=guest?'guest':'player';select.append(option);}
    if(choices.has(game.id))select.value=choices.get(game.id);
    play.onclick=()=>start(game,play,select,card);actions.append(play,select);card.append(actions);
    const react=el('button','React to this game','react-button');react.onclick=()=>openReaction(game);card.append(react);
    if(game.reactions.length){const chips=el('div',undefined,'chips');for(const [emoji]of emojis){const count=game.reactions.filter(r=>r.emoji===emoji).length;if(count)chips.append(el('span',emoji+' '+count,'chip'));}card.append(chips);}
    games.append(card);
  }
}
async function start(game,button,select,card) {
  const guest=select.value==='guest';
  // Open synchronously in the click event so browser popup blockers allow Play.
  const tab = window.open('about:blank','_blank');
  if(!tab){notice('Allow this site to open a game tab, then press Play again.');return;}
  tab.document.title='Opening '+game.title;tab.document.body.textContent='Getting '+game.title+' ready…';
  card.dataset.launching='true';button.disabled=true;button.textContent='Starting…';
  try {
    const launch=await api(`/api/apps/${game.id}/launch`,{method:'POST',body:JSON.stringify({guest})});
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
  $('members').replaceChildren();for(const[email,m]of Object.entries(members.members)){const row=el('div');row.append(el('span',`${m.name} · ${email}${m.admin?' · admin':''}${m.enabled?'':' · disabled'}`));if(!m.admin){const toggle=el('button',m.enabled?'Disable':'Enable','secondary');toggle.onclick=async()=>{try{await api('/api/admin/members',{method:'POST',body:JSON.stringify({email,...m,enabled:!m.enabled})});await adminRefresh();}catch(error){showError(error);}};row.append(toggle);}$('members').append(row);}
  $('credentials').replaceChildren();for(const c of credentials.credentials){const row=el('div'),button=el('button','Revoke','secondary');row.append(el('span',c.label+' · '+c.app),button);button.onclick=async()=>{try{await api('/api/admin/credentials/'+c.id,{method:'DELETE'});await adminRefresh();}catch(error){showError(error);}};$('credentials').append(row);}
}
$('invite-form').onsubmit=async event=>{event.preventDefault();const data=Object.fromEntries(new FormData(event.target));try{await api('/api/admin/members',{method:'POST',body:JSON.stringify(data)});event.target.reset();notice('Invitation saved. They can sign in with that Google account.');await adminRefresh();}catch(error){showError(error);}};
$('credential-form').onsubmit=async event=>{event.preventDefault();try{const data=Object.fromEntries(new FormData(event.target)),connection=await api('/api/admin/credentials',{method:'POST',body:JSON.stringify(data)});const blob=new Blob([JSON.stringify(connection,null,2)+'\n'],{type:'application/json'}),url=URL.createObjectURL(blob),link=el('a');link.href=url;link.download=connection.app_id+'.credentials.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);notice('Connection file downloaded. Attach it in your game’s existing agent chat.');await adminRefresh();}catch(error){showError(error);}};
try {
  me=await api('/api/me');$('welcome').hidden=true;$('clubhouse').hidden=false;
  $('greeting').textContent='Signed in as '+me.name+'.';$('account').append(el('span',me.name));const out=el('a','Sign out');out.href='/oauth2/sign_out?rd=%2F';$('account').append(out);
  await refresh();setInterval(()=>{if(!document.hidden)refresh();},25000);
  if(me.admin){$('admin').hidden=false;await adminRefresh();}
} catch(error) {if(error.status!==401)$('login-message').textContent=error.message;}

let newGameConnection, newGameTitle;
function showSetupPrompt() {
  $('agent-prompt').value=`Publish the game in this existing project to Infuser. Continue this project; do not replace my game or create a new app.

Read https://github.com/esper256/infuser/blob/main/docs/agents.md and use the recipe for the engine already in this project. Preserve the gameplay, art, and project structure. My game is called “${newGameTitle}”, and its Infuser app ID is ${newGameConnection.app_id}.

ONE-TIME SETUP CODE (expires after one hour):
${newGameConnection.setup_code}

Get the current Infuser CLI from the repository's release instructions. Feed the setup code to 'infuser connect --server ${newGameConnection.api_url} --app ${newGameConnection.app_id}' on stdin. It exchanges the code once and saves the permanent game-scoped credentials under .local/. Keep .local/ and *.credentials.json out of Git and game exports. Do not print or echo setup tokens or permanent credentials. If this project already has working credentials for this app ID, reuse them.

Available tag IDs in this Infuser: ${(newGameConnection.tag_catalog||[]).map(t=>t.id).join(', ')}. Choose a few fitting default tags from the game’s design and code; prefer these existing IDs. Put them in infuser.game.json. Query the current tag catalog after connecting if needed.

Inspect the engine and build scripts, declare the game's actual Single player / Multiplayer capabilities, integrate cloud saves and isolated guest play, build, publish, and verify the result. Handle routine setup yourself. Do not ask me to configure Docker, DNS, ports, or hosting. If the existing multiplayer transport is unsupported, explain the exact blocker instead of downgrading it.

Future updates must reuse the saved connection and the same app ID. Tell me when the game is ready to play in my library.`;
  $('add-game-title').textContent='Ready for your agent';
  $('setup-expiry').textContent='One use · expires '+new Date(newGameConnection.expires_at*1000).toLocaleTimeString([],{hour:'numeric',minute:'2-digit'})+'.';
  $('agent-kit').hidden=false;$('add-game-form').hidden=true;$('copy-prompt').textContent='Copy prompt';
}
$('add-game').onclick=()=>{$('add-game-dialog').showModal();};
$('close-add-game').onclick=()=>$('add-game-dialog').close();
$('add-game-form').onsubmit=async event=>{event.preventDefault();const button=event.target.querySelector('button');button.disabled=true;try{newGameTitle=new FormData(event.target).get('title');newGameConnection=await api('/api/my-games',{method:'POST',body:JSON.stringify({title:newGameTitle})});showSetupPrompt();}catch(error){showError(error);}finally{button.disabled=false;}};
$('copy-prompt').onclick=async()=>{try{if(Date.now()>=newGameConnection.expires_at*1000){newGameConnection=await api('/api/my-games/'+newGameConnection.app_id+'/setup',{method:'POST'});showSetupPrompt();}await navigator.clipboard.writeText($('agent-prompt').value);$('copy-prompt').textContent='Copied — paste it into your game’s chat';}catch{ $('agent-prompt').focus();$('agent-prompt').select();$('copy-prompt').textContent='Select and copy the prompt above';}};
$('refresh-setup').onclick=async()=>{try{newGameConnection=await api('/api/my-games/'+newGameConnection.app_id+'/setup',{method:'POST'});showSetupPrompt();}catch(error){showError(error);}};
$('another-game').onclick=()=>{newGameConnection=null;$('add-game-title').textContent='Add your game';$('agent-kit').hidden=true;$('add-game-form').hidden=false;$('add-game-form').reset();$('add-game-form').elements.title.focus();};
function tagLabel(id){return library?.tags?.find(t=>t.id===id)?.label||id.split('-').map(w=>w[0]?.toUpperCase()+w.slice(1)).join(' ');}
function renderFilters(){const nav=$('tag-filters');nav.replaceChildren();const all=el('button','All games','tag-filter'+(!activeTag?' selected':''));all.setAttribute('aria-pressed',String(!activeTag));all.onclick=()=>{activeTag='';renderFilters();renderGames();};nav.append(all);for(const tag of library.tags||[]){if(!tag.count&&activeTag!==tag.id)continue;const button=el('button',tag.label+' '+tag.count,'tag-filter'+(activeTag===tag.id?' selected':''));button.setAttribute('aria-pressed',String(activeTag===tag.id));button.onclick=()=>{activeTag=activeTag===tag.id?'':tag.id;renderFilters();renderGames();};nav.append(button);}}
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
