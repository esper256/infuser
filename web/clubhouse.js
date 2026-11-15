const $ = id => document.getElementById(id);
let me, library, currentReaction, refreshing = false;
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
    $('friends').textContent = library.friends.map(f => f.name).join(' · ') || 'You have the place to yourself for now.';
    renderGames(); renderFeed();
  } catch (error) { showError(error); } finally { refreshing = false; }
}
function renderGames() {
  const games = $('games');
  // Preserve ongoing launch controls and selections during presence polling.
  if (games.querySelector('[data-launching]')) return;
  const choices = new Map([...games.querySelectorAll('select')].map(s => [s.dataset.app,s.value]));
  games.replaceChildren();
  if(!library.apps.length) {
    const empty = el('div',undefined,'empty'); empty.append(el('h2','Your first world is on its way.'),el('p',me.admin ? 'Open the grown-up corner below to give a game-making agent its connection file. Published games will appear here automatically.' : 'Someone is making something for you. Come back soon!')); games.append(empty);
  }
  for(const game of library.apps) {
    const card = el('article',undefined,'game');
    card.append(el('div',game.play_modes.includes('multiplayer')?'✦':'◈','game-mark'),el('h2',game.title));
    const modes = game.play_modes.map(m => m === 'solo' ? 'Solo adventures' : 'Play together').join(' + ');
    card.append(el('p',(game.preview ? 'ADMIN PREVIEW · ' : '') + modes,'quiet'));
    const names = [...new Set(game.players.map(p => p.name + (p.guest ? '’s guest' : '')))];
    card.append(el('p',names.length ? names.join(', ') + ' playing now' : 'Ready when you are.','players'));
    const actions=el('div',undefined,'actions'), play=el('button','Play'), select=el('select'); select.dataset.app=game.id;select.setAttribute('aria-label','Play options for '+game.title);
    for (const mode of game.play_modes) for(const guest of [false,true]) {const option=el('option',guest ? `Guest mode · ${mode === 'solo'?'solo':'together'}` : mode==='solo'?'Solo':'Play together');option.value=mode+(guest?':guest':'');select.append(option);}
    if(choices.has(game.id))select.value=choices.get(game.id);
    play.onclick=()=>start(game,play,select,card);actions.append(play,select);card.append(actions);
    const react=el('button','＋ Send a reaction','react-button');react.onclick=()=>openReaction(game);card.append(react);
    if(game.reactions.length){const chips=el('div',undefined,'chips');for(const [emoji]of emojis){const count=game.reactions.filter(r=>r.emoji===emoji).length;if(count)chips.append(el('span',emoji+' '+count,'chip'));}card.append(chips);}
    games.append(card);
  }
}
async function start(game,button,select,card) {
  const [mode,guest]=select.value.split(':');
  // Open synchronously in the click event so browser popup blockers allow Play.
  const tab = window.open('about:blank','_blank');
  if(!tab){notice('Allow this site to open a game tab, then press Play again.');return;}
  tab.document.title='Opening '+game.title;tab.document.body.textContent='Getting '+game.title+' ready…';
  card.dataset.launching='true';button.disabled=true;button.textContent='Starting…';
  try {
    const launch=await api(`/api/apps/${game.id}/launch`,{method:'POST',body:JSON.stringify({mode,guest:!!guest})});
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
  for(const game of library.apps){entries.push({at:game.updated,emoji:'🌱',title:game.title+' has a fresh build',text:'Ready to explore.'});for(const r of game.reactions)entries.push({at:r.updated,emoji:r.emoji,title:r.name+' reacted to '+game.title,text:r.text});}
  entries.sort((a,b)=>b.at-a.at);
  if(!entries.length)feed.append(el('p','New games and little celebrations will land here.','quiet'));
  for(const entry of entries.slice(0,12)){const row=el('div',undefined,'feed-row'),body=el('div');body.append(el('small',entry.title));if(entry.text)body.append(el('p',entry.text));row.append(el('span',entry.emoji,'feed-emoji'),body);feed.append(row);}
}
for(const [emoji,label]of emojis){const l=el('label'),radio=el('input');radio.type='radio';radio.name='emoji';radio.value=emoji;radio.required=true;radio.setAttribute('aria-label',label);l.append(radio,el('span',emoji));$('emoji').append(l);}
function openReaction(game){currentReaction=game;$('reaction-title').textContent=game.title;const mine=game.reactions.find(r=>r.mine);$('reaction-form').reset();if(mine){$('reaction-form').elements.emoji.value=mine.emoji;$('reaction-form').elements.text.value=mine.text;}$('remove-reaction').hidden=!mine;$('reaction-dialog').showModal();}
$('close-reaction').onclick=()=>$('reaction-dialog').close();
$('reaction-form').onsubmit=async event=>{event.preventDefault();try{const f=new FormData(event.target);await api(`/api/apps/${currentReaction.id}/reaction`,{method:'PUT',body:JSON.stringify({emoji:f.get('emoji'),text:f.get('text')})});$('reaction-dialog').close();await refresh();}catch(error){showError(error);}};
$('remove-reaction').onclick=async()=>{try{await api(`/api/apps/${currentReaction.id}/reaction`,{method:'DELETE'});$('reaction-dialog').close();await refresh();}catch(error){showError(error);}};
async function adminRefresh(){
  const [members,credentials]=await Promise.all([api('/api/admin/members'),api('/api/admin/credentials')]);
  $('members').replaceChildren();for(const[email,m]of Object.entries(members.members)){const row=el('div');row.append(el('span',`${m.name} · ${email}${m.admin?' · admin':''}${m.enabled?'':' · disabled'}`));if(!m.admin){const toggle=el('button',m.enabled?'Disable':'Enable','secondary');toggle.onclick=async()=>{try{await api('/api/admin/members',{method:'POST',body:JSON.stringify({email,...m,enabled:!m.enabled})});await adminRefresh();}catch(error){showError(error);}};row.append(toggle);}$('members').append(row);}
  $('credentials').replaceChildren();for(const c of credentials.credentials){const row=el('div'),button=el('button','Revoke','secondary');row.append(el('span',c.label+' · '+c.app),button);button.onclick=async()=>{try{await api('/api/admin/credentials/'+c.id,{method:'DELETE'});await adminRefresh();}catch(error){showError(error);}};$('credentials').append(row);}
}
$('invite-form').onsubmit=async event=>{event.preventDefault();const data=Object.fromEntries(new FormData(event.target));try{await api('/api/admin/members',{method:'POST',body:JSON.stringify(data)});event.target.reset();notice('Invitation saved. They can sign in with that Google account.');await adminRefresh();}catch(error){showError(error);}};
$('credential-form').onsubmit=async event=>{event.preventDefault();try{const data=Object.fromEntries(new FormData(event.target)),connection=await api('/api/admin/credentials',{method:'POST',body:JSON.stringify(data)});const blob=new Blob([JSON.stringify(connection,null,2)+'\n'],{type:'application/json'}),url=URL.createObjectURL(blob),link=el('a');link.href=url;link.download=connection.app_id+'.credentials.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);notice('Connection file downloaded. Keep it out of the game repository.');await adminRefresh();}catch(error){showError(error);}};
try {
  me=await api('/api/me');$('welcome').hidden=true;$('clubhouse').hidden=false;
  $('greeting').textContent='Hey, '+me.name+'.';$('account').append(el('span',me.name));const out=el('a','Sign out');out.href='/oauth2/sign_out?rd=%2F';$('account').append(out);
  await refresh();setInterval(()=>{if(!document.hidden)refresh();},25000);
  if(me.admin){$('admin').hidden=false;await adminRefresh();}
} catch(error) {if(error.status!==401)$('login-message').textContent=error.message;}
let newGameConnection;
function downloadPass(connection) { const blob=new Blob([JSON.stringify(connection,null,2)+'\n'],{type:'application/json'}),url=URL.createObjectURL(blob),link=el('a');link.href=url;link.download=connection.app_id+'.credentials.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000); }
$('add-game').onclick=()=>{$('add-game-dialog').showModal();};
$('close-add-game').onclick=()=>$('add-game-dialog').close();
$('add-game-form').onsubmit=async event=>{event.preventDefault();const button=event.target.querySelector('button');button.disabled=true;try{const title=new FormData(event.target).get('title');newGameConnection=await api('/api/my-games',{method:'POST',body:JSON.stringify({title})});$('agent-prompt').value=`Help me put my game on Infuser!\n\nRead https://github.com/esper256/infuser/blob/main/docs/agents.md and follow the guide for this game's engine. I will give you ${newGameConnection.app_id}.credentials.json separately. Keep that game pass private and out of Git, builds, screenshots, and chat.\n\nInspect my existing game first. Use the easiest supported engine recipe; do not start by asking me to set up Docker. Preserve my game and make only the Infuser integration changes. Declare its actual solo/multiplayer modes explicitly, integrate cloud saves and guest mode, then build, publish, and verify it. My game should appear in the clubhouse as “${title}”. Explain any unsupported multiplayer requirement clearly instead of silently changing the game.\n\nFor future updates, use the same game pass and app ID.`;$('agent-kit').hidden=false;$('add-game-form').hidden=true;$('copy-prompt').textContent='Copy prompt';}catch(error){showError(error);}finally{button.disabled=false;}};
$('copy-prompt').onclick=async()=>{try{await navigator.clipboard.writeText($('agent-prompt').value);$('copy-prompt').textContent='Copied! Go make something great ✨';}catch{ $('agent-prompt').focus();$('agent-prompt').select();$('copy-prompt').textContent='Select and copy the prompt above';}};
$('download-pass').onclick=()=>downloadPass(newGameConnection);
