// Browser-composited capture only. No game hooks or canvas-reading assumptions.
export function initScreenshotCapture({button,viewport,message}) {
  let busy=false,stream,alive=true;
  const stop=()=>{stream?.getTracks().forEach(t=>t.stop());stream=null;};
  addEventListener('pagehide',()=>{alive=false;stop();});
  const blob=(canvas,quality)=>new Promise((resolve,reject)=>canvas.toBlob(value=>value?.type==='image/webp'?resolve(value):reject(Error('This browser cannot encode screenshots.')),'image/webp',quality));
  const encoded=value=>new Promise((resolve,reject)=>{const reader=new FileReader();reader.onload=()=>resolve(reader.result.split(',')[1]);reader.onerror=reject;reader.readAsDataURL(value);});
  button.addEventListener('click',async()=>{
    if(busy||button.disabled)return;
    if(!navigator.mediaDevices?.getDisplayMedia||!globalThis.CropTarget){message('Screenshot capture is not available in this browser. Try desktop Chrome or Edge.');return;}
    busy=true;button.disabled=true;button.setAttribute('aria-busy','true');
    let video,timer;
    try{
      // Invoke permission directly from the click; nothing is captured before approval.
      stream=await navigator.mediaDevices.getDisplayMedia({video:{displaySurface:'browser',width:{ideal:4096},height:{ideal:4096}},audio:false,preferCurrentTab:true,selfBrowserSurface:'include',surfaceSwitching:'exclude',monitorTypeSurfaces:'exclude'});
      if(!alive)return;
      const track=stream.getVideoTracks()[0];
      if(!track?.cropTo)throw Error('This browser cannot capture just the game. Nothing was saved.');
      // This also rejects a different tab/window: never auto-save an arbitrary desktop.
      try{await track.cropTo(await CropTarget.fromElement(viewport));}
      catch{throw Error('Choose this game’s tab in the sharing picker. Nothing was saved.');}
      video=document.createElement('video');video.muted=true;video.playsInline=true;video.srcObject=stream;
      await Promise.race([(async()=>{await video.play();await new Promise(resolve=>video.requestVideoFrameCallback(resolve));})(),new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error('The browser did not provide a screenshot. Try again.')),6000);})]);
      clearTimeout(timer);
      if(!alive)return;
      const rawWidth=video.videoWidth,rawHeight=video.videoHeight;
      if(!rawWidth||!rawHeight||rawWidth>8192||rawHeight>8192||rawWidth*rawHeight>33554432)throw Error('That capture is too large. Try a smaller game window.');
      const ratio=Math.min(1,4096/rawWidth,4096/rawHeight,Math.sqrt(8294400/(rawWidth*rawHeight)));
      const width=Math.max(1,Math.floor(rawWidth*ratio)),height=Math.max(1,Math.floor(rawHeight*ratio));
      const canvas=document.createElement('canvas');canvas.width=width;canvas.height=height;
      canvas.getContext('2d',{alpha:false}).drawImage(video,0,0,width,height);stop();video.srcObject=null;
      let image;
      for(const quality of [.8,.65,.5,.35]){image=await blob(canvas,quality);if(image.size<=6*1024*1024)break;}
      if(image.size>6*1024*1024)throw Error('That screenshot is too large to save.');
      const thumb=document.createElement('canvas'),scale=Math.min(1,320/width,320/height);
      thumb.width=Math.max(1,Math.round(width*scale));thumb.height=Math.max(1,Math.round(height*scale));thumb.getContext('2d',{alpha:false}).drawImage(canvas,0,0,thumb.width,thumb.height);
      const thumbnail=await blob(thumb,.65);
      if(!alive)return;
      message('Saving screenshot…');
      const response=await fetch('/_infuser/screenshots',{method:'POST',credentials:'same-origin',headers:{'Content-Type':'application/json'},body:JSON.stringify({id:crypto.randomUUID(),image:await encoded(image),thumbnail:await encoded(thumbnail)}),signal:AbortSignal.timeout(20000)});
      const result=await response.json();if(!response.ok)throw Error(result.error||'Could not save the screenshot.');
      message('Screenshot saved. Attach it in chat or a game reaction.');
    }catch(error){
      if(alive)message(error.name==='NotAllowedError'||error.name==='AbortError'?'Screenshot not saved.':error.message||'Could not capture this game.');
    }finally{clearTimeout(timer);stop();if(video)video.srcObject=null;busy=false;button.disabled=!alive||!!viewport.querySelector('iframe')?.hidden;button.removeAttribute('aria-busy');}
  });
}
