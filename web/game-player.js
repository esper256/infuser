import { initScreenshotCapture } from '/_infuser/screenshot-capture.js';
const title=document.getElementById('title'),game=document.getElementById('game'),status=document.getElementById('status'),button=document.getElementById('fullscreen');
function message(text){status.textContent=text;status.hidden=!text;}
initScreenshotCapture({button:document.getElementById('screenshot'),viewport:document.getElementById('viewport'),message});
const mobile = matchMedia('(pointer: coarse)');
const rotateHint = document.getElementById('rotate-hint');
let gameOrientation;
let requestedLock = false;
function applyOrientation() {
  if (!gameOrientation || document.hidden || !mobile.matches) return;
  rotateHint.dataset.orientation = gameOrientation;
  rotateHint.textContent = gameOrientation === 'portrait' ? 'Rotate your device upright' : 'Rotate your device sideways';
  if (screen.orientation?.lock) {
    requestedLock = true;
    // Some browsers allow this on launch, others only after entering fullscreen.
    screen.orientation.lock(gameOrientation).catch(() => {});
  }
}
function releaseOrientation() {
  delete rotateHint.dataset.orientation;
  if (requestedLock) {
    requestedLock = false;
    try { screen.orientation?.unlock?.(); } catch { /* Browser already released it. */ }
  }
}
addEventListener('pagehide', releaseOrientation);
addEventListener('pageshow', event => { if (event.persisted) applyOrientation(); });
document.addEventListener('visibilitychange', () => { if (!document.hidden) applyOrientation(); });
mobile.addEventListener('change', () => mobile.matches ? applyOrientation() : releaseOrientation());
button.addEventListener('click',async()=>{
  try{
    if(document.fullscreenElement)await document.exitFullscreen();
    else if(document.documentElement.requestFullscreen)await document.documentElement.requestFullscreen();
    else{message('Fullscreen isn’t available in this browser.');return;}
    message('');game.contentWindow.focus();
  }catch{message('This browser blocked fullscreen. Try opening the game in Chrome.');}
});
document.addEventListener('fullscreenchange',()=>{
  if (document.fullscreenElement) applyOrientation();
  else releaseOrientation();
  button.querySelector('span').textContent=document.fullscreenElement?'Exit fullscreen':'Fullscreen';
  button.setAttribute('aria-label',document.fullscreenElement?'Exit fullscreen':'Enter fullscreen');
});
try{
  const response=await fetch('/_infuser/context',{credentials:'same-origin'});
  const session=await response.json();
  if(!response.ok)throw Error(session.error||'Open this game again from your Library.');
  gameOrientation = session.manifest.orientation === 'portrait' ? 'portrait' : 'landscape';
  applyOrientation();
  title.textContent=session.manifest.title;document.title=session.manifest.title+' · Infuser';
  document.getElementById('library').href=session.library_url;
  document.getElementById('guest').hidden=!session.guest;
  // A separate viewport keeps fixed canvases and 100vh layouts below the bar.
  // Both documents stay on the existing isolated game origin; no portal cookies.
  game.addEventListener('load',()=>{document.getElementById('screenshot').disabled=false;game.contentWindow.focus();},{once:true});
  game.src='/'+location.search;game.hidden=false;
  if(session.visit){
    let checked=performance.now(),checkedWall=Date.now(),ended=false,checking=false;
    const end=()=>{if(ended)return;ended=true;gameOrientation=null;releaseOrientation();game.removeAttribute('src');game.hidden=true;button.disabled=true;document.getElementById('screenshot').disabled=true;message('This visit has ended. Return to your Library.');};
    const watch=()=>{if(Math.max(performance.now()-checked,Date.now()-checkedWall)>=30000)end();};
    async function check(){
      watch();if(ended||checking)return;checking=true;
      try{
        const r=await fetch('/_infuser/api/v1/heartbeat',{method:'POST',credentials:'same-origin',signal:AbortSignal.timeout(5000)});
        if([401,403,404,410].includes(r.status)){end();return;}
        if(r.ok){const state=await r.json();watch();if(!ended){checked=performance.now();checkedWall=Date.now();if(state.visit){const left=Math.ceil((state.visit.expires_at-Date.now()/1000)/60);message(left<=5?`Visit ends in ${Math.max(0,left)} min. Save your progress.`:'');}}}
      }catch{watch();}finally{checking=false;}
    }
    setInterval(watch,500);setInterval(check,5000);
    document.addEventListener('visibilitychange',()=>{watch();if(!document.hidden)check();});
  }
}catch(error){title.textContent='Game unavailable';message(error.message);}
const connection=document.createElement('span');connection.id='connection';connection.hidden=true;
connection.setAttribute('role','img');button.before(connection);
window.addEventListener('message',event=>{
  if(event.origin!==location.origin||event.source!==game.contentWindow||event.data?.type!=='infuser-network')return;
  const descriptions={connected:'Connected',connecting:'Connecting',ended:'Multiplayer disconnected'};
  const state=event.data.status;if(!descriptions[state])return;
  connection.hidden=false;connection.textContent=state==='connected'?'●':state==='connecting'?'◌':'○';
  connection.dataset.state=state;connection.title=descriptions[state];connection.setAttribute('aria-label',descriptions[state]);
});
