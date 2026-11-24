import { clientUpdateAction } from '/_infuser/client-updates.js';
import { watchInput } from '/_infuser/activity-input.js';
import { initScreenshotCapture } from '/_infuser/screenshot-capture.js';
// Capture fetch and consume the per-page voice token before the game iframe loads.

const voiceTokenMeta = document.querySelector('meta[name="infuser-voice-token"]');
const playerVoiceToken = voiceTokenMeta?.content || '';
voiceTokenMeta?.remove();
const title=document.getElementById('title'),game=document.getElementById('game'),status=document.getElementById('status'),button=document.getElementById('fullscreen');
const viewport=document.getElementById('viewport'),resolutionButton=document.getElementById('resolution');
const MAX_RENDER_PIXELS=1920*1080;
let hdCap=true,currentAppId=null,diagnostics;
window.infuserHdCap=()=>hdCap;
try{const {recorder}=await import('/_infuser/telemetry.js');diagnostics=recorder({source:'player',endpoint:'/_infuser/telemetry',scope:document.documentElement.dataset.diagnosticLaunch});}catch{}
const playerFetch=window.fetch.bind(window);
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
let sessionEnded=false;
let updateMessage='',visitMessage='';
function message(text){
  if(sessionEnded)return;
  const notice=visitMessage||text||updateMessage;
  status.textContent=notice;status.hidden=!notice;
}
const header = document.getElementById('player-header');
const edge = document.getElementById('controls-edge');
const playerFullscreen = () => document.fullscreenElement === document.documentElement;
let hideTimer;
let swipe;
let keyboardControls = false;
let voiceMenuOpen = false;

function hideControls() {
  clearTimeout(hideTimer);
  if (sessionEnded) { showControls(); return; }
  if (voiceMenuOpen) return;
  header.removeAttribute('data-open');
  header.inert = playerFullscreen();
  if (playerFullscreen() && header.contains(document.activeElement)) game.focus();
}
function showControls() {
  if (!playerFullscreen()) return;
  clearTimeout(hideTimer);
  header.inert = false;
  header.toggleAttribute('data-open', true);
  if (!sessionEnded && !keyboardControls && !voiceMenuOpen && !game.hidden) hideTimer = setTimeout(hideControls, 3500);
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
    playerFetch('/_infuser/player-pages/'+page+'/close', {
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
  const response=await playerFetch('/_infuser/context',{credentials:'same-origin'});
  const session=await response.json();
  if(!response.ok)throw Error(session.error||'Open this game again from your Library.');
  diagnostics?.link('launch').set('up');
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
  // A separate viewport keeps fixed canvases and 100vh layouts below the bar.
  // Both documents stay on the existing isolated game origin; no portal cookies.
  if(!session.guest){
    if(playerVoiceToken){
      void import('/_infuser/player-voice.js').then(({ initGameVoice }) => {
        initGameVoice({
          header,
          anchorBefore: resolutionButton,
          voiceToken: playerVoiceToken,
          fetchFn: playerFetch,
          onPopoverChange(isOpen) {
            voiceMenuOpen = isOpen;
            if (isOpen) {
              clearTimeout(hideTimer);
              if (playerFullscreen()) showControls();
            } else if (playerFullscreen() && header.hasAttribute('data-open')) {
              showControls();
            }
          },
          onCloseFocus() {
            if (!game.hidden) game.contentWindow?.focus();
          },
        });
      }).catch(() => { /* Voice controls are optional and must never block game launch. */ });
    }
    const report=async()=>{if(!sessionEnded)await playerFetch('/_infuser/input',{method:'POST',credentials:'same-origin'});};
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
  const hookTimer=setInterval(()=>{if(game.contentDocument&&game.contentDocument!==initialDoc){diagnostics?.hook(game.contentWindow,'game');tuneGraphics(game.contentWindow);}},5);
  let watchTimer,checkTimer;
  // A missing or unrecognized launch (401/404) can already have been cleaned up.
  // Keep the observed status without guessing why it disappeared.
  const heartbeatEndReasons=new Map([[401,undefined],[403,session.visit?'visit_end':'access_lost'],
    [404,undefined],[409,'server_ended'],[410,'timeout']]);
  function endSession(reason,httpStatus){
    if(sessionEnded)return;
    sessionEnded=true;
    diagnostics?.link('launch').set('ended',reason,{status:httpStatus});
    if(session.visit){
      clearInterval(hookTimer);clearInterval(watchTimer);clearInterval(checkTimer);
      gameOrientation=null;releaseOrientation();game.hidden=true;game.removeAttribute('src');
      button.disabled=true;document.getElementById('screenshot').disabled=true;
    }
    const back=document.createElement('a');
    back.href=session.library_url;back.textContent='Return to your Library.';
    status.replaceChildren(session.visit?'This visit has ended. ':'This game session ended. ',back);
    status.hidden=false;showControls();
  }
  const stamp=document.querySelector('meta[name="infuser-client-version"]');
  let reloading=false,gameUpdate=false,platformUpdate=false;
  function updateNotice(){
    const showingUpdate=status.hidden || status.textContent===updateMessage;
    const title=session.manifest.title;
    if(gameUpdate && platformUpdate)updateMessage=`Infuser and ${title} have been updated. Reopen the game at your next break.`;
    else if(gameUpdate)updateMessage=`A new version of ${title} is ready. Reopen it at your next break.`;
    else if(platformUpdate)updateMessage='Infuser has been updated. Reopen this game at your next break.';
    else updateMessage='';
    if(showingUpdate)message('');
  }
  function clientUpdate(state){
    if(!state || reloading)return;
    const action=clientUpdateAction(stamp,state);
    if(action==='reload'){
      reloading=true;diagnostics?.markUpdate();location.reload();
    }else if(platformUpdate!==(action==='available')){
      platformUpdate=action==='available';updateNotice();
    }
  }
  function updateAvailable(){
    if(gameUpdate)return;
    gameUpdate=true;updateNotice();
  }
  clientUpdate(session.runtime?.client_update);
  if(session.runtime?.update_available)updateAvailable();
  window.addEventListener('message',event=>{
    if(event.origin!==location.origin || event.source!==game.contentWindow)return;
    if(event.data?.type==='infuser-client-update'){clientUpdate(event.data);return;}
    if(event.data?.type==='infuser-update-available'){updateAvailable();return;}
    if(event.data?.type!=='infuser-session-error')return;
    const {status}=event.data;
    if(heartbeatEndReasons.has(status))endSession(heartbeatEndReasons.get(status),status);
  });
  game.addEventListener('load',()=>{clearInterval(hookTimer);if(sessionEnded&&session.visit)return;diagnostics?.hook(game.contentWindow,'game');diagnostics?.loaded();tuneGraphics(game.contentWindow);fitViewport();document.getElementById('screenshot').disabled=false;game.contentWindow.focus();},{once:true});
  game.hidden=false;fitViewport();game.src='/'+location.search;
  if(session.visit){
    let checked=performance.now(),checkedWall=Date.now(),checking=false;
    const watch=()=>{if(Math.max(performance.now()-checked,Date.now()-checkedWall)>=30000)endSession('timeout');};
    async function check(){
      watch();if(sessionEnded||checking)return;checking=true;
      try{
        const r=await playerFetch('/_infuser/api/v1/heartbeat',{method:'POST',credentials:'same-origin',signal:AbortSignal.timeout(5000)});
        if(heartbeatEndReasons.has(r.status)){endSession(heartbeatEndReasons.get(r.status),r.status);return;}
        if(r.ok){
          const state=await r.json();watch();if(sessionEnded)return;
          checked=performance.now();checkedWall=Date.now();
          if(state.visit){
            const left=Math.ceil((state.visit.expires_at-Date.now()/1000)/60);
            visitMessage=left<=5?`Visit ends in ${Math.max(0,left)} min. Save your progress.`:'';
            message('');
          }
          clientUpdate(state.client_update);
          if(state.update_available)updateAvailable();
        }
      }catch{watch();}finally{checking=false;}
    }
    watchTimer=setInterval(watch,500);checkTimer=setInterval(check,5000);
    document.addEventListener('visibilitychange',()=>{watch();if(!document.hidden)check();});
  }
}catch(error){diagnostics?.loadFailed(error);title.textContent='Game unavailable';message(error.message);}
const connection=document.createElement('span');connection.id='connection';connection.hidden=true;
connection.setAttribute('role','img');button.before(connection);
window.addEventListener('message',event=>{
  if(event.origin!==location.origin||event.source!==game.contentWindow||event.data?.type!=='infuser-network')return;
  const descriptions={connected:'Connected',connecting:'Connecting',ended:'Multiplayer disconnected'};
  const state=event.data.status;if(!descriptions[state])return;
  diagnostics?.multiplayer(state);connection.hidden=false;connection.textContent=state==='connected'?'●':state==='connecting'?'◌':'○';
  connection.dataset.state=state;connection.title=descriptions[state];connection.setAttribute('aria-label',descriptions[state]);
});
