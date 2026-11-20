import { watchInput } from '/_infuser/activity-input.js';
import { initScreenshotCapture } from '/_infuser/screenshot-capture.js';
const title=document.getElementById('title'),game=document.getElementById('game'),status=document.getElementById('status'),button=document.getElementById('fullscreen');
const viewport=document.getElementById('viewport'),resolutionButton=document.getElementById('resolution');
const MAX_RENDER_PIXELS=1920*1080;
let hdCap=true,currentAppId=null;
window.infuserHdCap=()=>hdCap;
function resolutionChoiceMatters(){
  const rect=viewport.getBoundingClientRect(),area=rect.width*rect.height;
  if(!(area>0))return true;
  const factor=Math.max(1,window.devicePixelRatio||1);
  return area*factor*factor>MAX_RENDER_PIXELS;
}
function syncResolutionButton(){
  if(!resolutionButton)return;
  resolutionButton.hidden=!resolutionChoiceMatters();
  resolutionButton.textContent=hdCap?'HD':'MAX';
  resolutionButton.dataset.mode=hdCap?'hd':'max';
  resolutionButton.removeAttribute('aria-pressed');
  const label=hdCap?'Rendering HD (1080p) — switch to MAX':'Rendering MAX (native) — switch to HD';
  resolutionButton.setAttribute('aria-label',label);
  resolutionButton.title=label;
}
function tuneGraphics(win){
  if(!win||win.infuserGraphicsTuned)return;
  try{
    win.infuserGraphicsTuned=true;
    const dpr=Object.getOwnPropertyDescriptor(win,'devicePixelRatio')||Object.getOwnPropertyDescriptor(Object.getPrototypeOf(win),'devicePixelRatio');
    const getDpr=dpr?.get?()=>dpr.get.call(win):()=>window.devicePixelRatio||1;
    Object.defineProperty(win,'devicePixelRatio',{
      configurable:true,
      get(){
        const raw=getDpr()||1;
        if(!hdCap)return raw;
        const area=Math.max(1,(win.innerWidth||1920)*(win.innerHeight||1080));
        return Math.min(raw,Math.max(0.5,Math.sqrt(MAX_RENDER_PIXELS/area)));
      }
    });
    const getContext=win.HTMLCanvasElement?.prototype.getContext;
    if(getContext&&!getContext.infuserWrapped){
      const wrapped=function(type,attributes){
        if((type==='webgl'||type==='webgl2')&&(!attributes||attributes.powerPreference===undefined)){
          attributes={...attributes,powerPreference:'high-performance'};
        }
        return getContext.call(this,type,attributes);
      };
      wrapped.infuserWrapped=true;
      win.HTMLCanvasElement.prototype.getContext=wrapped;
    }
  }catch{/* Same-origin only; ignore transient navigation states. */}
}
let lastScaled=false;
function fitViewport(){
  const rect=viewport.getBoundingClientRect(),vw=rect.width,vh=rect.height;
  if(hdCap&&vw>0&&vh>0&&vw*vh>MAX_RENDER_PIXELS){
    const scale=Math.sqrt(MAX_RENDER_PIXELS/(vw*vh));
    const w=Math.max(1,Math.round(vw*scale)),h=Math.max(1,Math.round(vh*scale));
    game.style.width=`${w}px`;game.style.height=`${h}px`;
    game.style.transformOrigin='0 0';game.style.transform=`scale(${vw/w},${vh/h})`;
    lastScaled=true;
  }else if(lastScaled){
    game.style.width='';game.style.height='';game.style.transformOrigin='';game.style.transform='';
    lastScaled=false;
  }
}
resolutionButton?.addEventListener('click',()=>{
  if(!resolutionChoiceMatters())return;
  hdCap=!hdCap;
  if(currentAppId){try{localStorage.setItem('infuser.hd.'+currentAppId,hdCap?'1':'0');}catch{}}
  syncResolutionButton();
  fitViewport();
  try{game.contentWindow?.dispatchEvent(new Event('resize'));}catch{}
  if(!game.hidden)game.contentWindow?.focus();
});
function onViewportResize(){syncResolutionButton();fitViewport();}
if(typeof ResizeObserver!=='undefined')new ResizeObserver(onViewportResize).observe(viewport);
addEventListener('resize',onViewportResize);
function message(text){status.textContent=text;status.hidden=!text;}
const header = document.getElementById('player-header');
const edge = document.getElementById('controls-edge');
const playerFullscreen = () => document.fullscreenElement === document.documentElement;
let hideTimer;
let swipe;
let keyboardControls = false;

function hideControls() {
  clearTimeout(hideTimer);
  header.removeAttribute('data-open');
  header.inert = playerFullscreen();
  if (playerFullscreen() && header.contains(document.activeElement)) game.focus();
}
function showControls() {
  if (!playerFullscreen()) return;
  clearTimeout(hideTimer);
  header.inert = false;
  header.toggleAttribute('data-open', true);
  if (!keyboardControls && !game.hidden) hideTimer = setTimeout(hideControls, 3500);
}
// Capture just the top edge. The game iframe owns gestures everywhere else.
edge.addEventListener('pointerdown', event => {
  if (!event.isPrimary || event.button !== 0) return;
  swipe = {id:event.pointerId, x:event.clientX, y:event.clientY};
  edge.setPointerCapture(event.pointerId);
});
edge.addEventListener('pointermove', event => {
  if (swipe?.id !== event.pointerId) return;
  const down = event.clientY - swipe.y;
  if (down >= 24 && down > Math.abs(event.clientX - swipe.x)) {
    swipe = null;
    showControls();
  }
});
edge.addEventListener('lostpointercapture', () => { swipe = null; });
edge.addEventListener('pointerenter', event => {
  if (event.pointerType === 'mouse' && !event.buttons) showControls();
});
edge.addEventListener('click', event => {
  if (event.detail !== 0) return;
  keyboardControls = true;
  showControls();
  document.getElementById('library').focus();
});
header.addEventListener('keydown', () => { keyboardControls = true; clearTimeout(hideTimer); });
header.addEventListener('focusout', event => {
  if (keyboardControls && !header.contains(event.relatedTarget)) {
    keyboardControls = false;
    if (header.hasAttribute('data-open')) showControls();
  }
});
header.addEventListener('pointerdown', () => { keyboardControls = false; showControls(); });
header.addEventListener('pointermove', showControls);
document.addEventListener('fullscreenchange', () => {
  keyboardControls = false;
  edge.hidden = !playerFullscreen();
  hideControls();
});
initScreenshotCapture({
  button:document.getElementById('screenshot'), viewport:document.getElementById('viewport'),
  message: text => { message(text); showControls(); }
});
const mobile = matchMedia('(pointer: coarse)');
const rotateHint = document.getElementById('rotate-hint');
let gameOrientation;
let requestedLock = false;
let waitingForTurn = false;
let turnHandled = false;

function currentOrientation() {
  const type = screen.orientation?.type;
  if (type) return type.split('-')[0];
  if (typeof window.orientation === 'number') return Math.abs(window.orientation) === 90 ? 'landscape' : 'portrait';
  return matchMedia('(orientation: portrait)').matches ? 'portrait' : 'landscape';
}
function lockOrientation() {
  if (!gameOrientation || !mobile.matches || !screen.orientation?.lock) return;
  requestedLock = true;
  screen.orientation.lock(gameOrientation).catch(() => {});
}
function releaseOrientation() {
  rotateHint.hidden = true;
  if (requestedLock) {
    requestedLock = false;
    try { screen.orientation?.unlock?.(); } catch { /* Browser already released it. */ }
  }
}
function syncOrientation(turned = false) {
  if (!gameOrientation || !mobile.matches || document.hidden || turnHandled) return;
  if (document.fullscreenElement) {
    turnHandled = true;
    lockOrientation();
    return;
  }
  if (currentOrientation() !== gameOrientation) {
    waitingForTurn = true;
    rotateHint.textContent = gameOrientation === 'portrait' ? 'Rotate your device upright' : 'Rotate your device sideways';
    rotateHint.hidden = false;
    return;
  }
  rotateHint.hidden = true;
  if (waitingForTurn && turned) {
    turnHandled = true;
    if (document.documentElement.requestFullscreen && document.fullscreenEnabled) {
      // Call directly in the rotation event so browsers can accept the gesture.
      document.documentElement.requestFullscreen().catch(() => {
        if (!gameOrientation || document.fullscreenElement || document.hidden) return;
        rotateHint.textContent = 'Tap Fullscreen to finish.';
        rotateHint.hidden = false;
        lockOrientation();
      });
    } else lockOrientation();
  } else if (!waitingForTurn) lockOrientation();
}
const onTurn = () => syncOrientation(true);
if (screen.orientation?.addEventListener) screen.orientation.addEventListener('change', onTurn);
else window.addEventListener('orientationchange', onTurn);
addEventListener('pagehide', event => {
  releaseOrientation();
  const page = document.documentElement.dataset.playerPage;
  if (!event.persisted && page) {
    // Keep the same-origin POST alive as the tab closes. The player response
    // suppresses referrers by default; the endpoint verifies the request Origin.
    fetch('/_infuser/player-pages/'+page+'/close', {
      method:'POST', keepalive:true, credentials:'same-origin', referrerPolicy:'same-origin'
    }).catch(()=>{}); // The heartbeat timeout covers missed departures.
  }
});
addEventListener('pageshow', event => { if (event.persisted) syncOrientation(); });
document.addEventListener('visibilitychange', () => { if (!document.hidden) syncOrientation(); });
mobile.addEventListener('change', () => mobile.matches ? syncOrientation() : releaseOrientation());
button.addEventListener('click',async()=>{
  try{
    if(document.fullscreenElement)await document.exitFullscreen();
    else if(document.documentElement.requestFullscreen)await document.documentElement.requestFullscreen();
    else{message('Fullscreen isn’t available in this browser.');return;}
    message('');game.contentWindow.focus();
  }catch{message('This browser blocked fullscreen. Try opening the game in Chrome.');}
});
document.addEventListener('fullscreenchange',()=>{
  turnHandled = true;
  rotateHint.hidden = true;
  if (document.fullscreenElement) lockOrientation();
  else releaseOrientation();
  button.querySelector('span').textContent=document.fullscreenElement?'Exit fullscreen':'Fullscreen';
  button.setAttribute('aria-label',document.fullscreenElement?'Exit fullscreen':'Enter fullscreen');
});
try{
  const response=await fetch('/_infuser/context',{credentials:'same-origin'});
  const session=await response.json();
  if(!response.ok)throw Error(session.error||'Open this game again from your Library.');
  currentAppId=session.app_id||null;
  let savedHd=null;
  if(currentAppId){try{savedHd=localStorage.getItem('infuser.hd.'+currentAppId);}catch{}}
  hdCap=savedHd==='1'?true:savedHd==='0'?false:session.manifest.resolution!=='native';
  syncResolutionButton();
  gameOrientation = session.manifest.orientation === 'portrait' ? 'portrait' : 'landscape';
  syncOrientation();
  title.textContent=session.manifest.title;document.title=session.manifest.title+' · Infuser';
  document.getElementById('library').href=session.library_url;
  document.getElementById('guest').hidden=!session.guest;
  const pathLabel=document.getElementById('path');
  async function refreshPath(){
    try{
      const response=await fetch('/_infuser/network-path',{credentials:'same-origin'});
      if(!response.ok){pathLabel.hidden=true;return;}
      const body=await response.json();
      const label=typeof body.label==='string'?body.label:'';
      pathLabel.hidden=!label;
      pathLabel.textContent=label;
    }catch{pathLabel.hidden=true;}
  }
  refreshPath();
  setInterval(refreshPath,5000);
  // A separate viewport keeps fixed canvases and 100vh layouts below the bar.
  // Both documents stay on the existing isolated game origin; no portal cookies.
  if(!session.guest){
    const report=()=>fetch('/_infuser/input',{method:'POST',credentials:'same-origin'});
    const padActive=watchInput(document,report);
    game.addEventListener('load',()=>{
      const doc=game.contentDocument;
      if(doc)watchInput(doc,report);
    });
    // Gamepad input does not produce DOM key or pointer events.
    let padTimer;
    function connectedPads(){
      try{return Array.from(navigator.getGamepads?.()||[]).filter(Boolean);}
      catch{return [];} // Some embedding policies disable gamepad access.
    }
    function checkPads(){
      if(document.hidden||!document.hasFocus())return;
      const pads=connectedPads();
      if(!pads.length){clearInterval(padTimer);padTimer=null;}
      if(pads.some(p=>p.buttons.some(b=>b.pressed)||p.axes.some(a=>Math.abs(a)>.25)))padActive();
    }
    window.addEventListener('gamepadconnected',()=>{if(!padTimer)padTimer=setInterval(checkPads,1000);});
    if(connectedPads().length)padTimer=setInterval(checkPads,1000);
  }
  const initialDoc=game.contentDocument;
  const hookTimer=setInterval(()=>{if(game.contentDocument&&game.contentDocument!==initialDoc)tuneGraphics(game.contentWindow);},5);
  game.addEventListener('load',()=>{clearInterval(hookTimer);tuneGraphics(game.contentWindow);fitViewport();document.getElementById('screenshot').disabled=false;game.contentWindow.focus();},{once:true});
  game.hidden=false;fitViewport();game.src='/'+location.search;
  if(session.visit){
    let checked=performance.now(),checkedWall=Date.now(),ended=false,checking=false;
    const end=()=>{if(ended)return;ended=true;gameOrientation=null;releaseOrientation();game.removeAttribute('src');game.hidden=true;button.disabled=true;document.getElementById('screenshot').disabled=true;showControls();message('This visit has ended. Return to your Library.');};
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
