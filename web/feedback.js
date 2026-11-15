// No logs, page/account/game data, storage, URLs, or error messages are collected.
export function initFeedback(button, closeMenu, api) {
  const dialog=document.createElement('dialog');dialog.id='feedback-dialog';dialog.setAttribute('aria-labelledby','feedback-title');
  dialog.innerHTML=`<button type="button" class="close" aria-label="Close feedback">×</button>
    <h2 id="feedback-title">Send feedback</h2>
    <p>This posts a <strong>public GitHub issue</strong> about Infuser. Anyone can read it. If you’re a child, ask an adult to help review it.</p>
    <p>Leave out names, email addresses, private game details, links, setup codes and passwords.</p>
    <form><label>Type<select name="kind"><option value="feature">Feature request</option><option value="bug">Bug</option></select></label>
    <label>Title<input name="title" maxlength="80" required autocomplete="off"></label>
    <label>What would you like to tell us?<textarea name="description" maxlength="2000" rows="5" required autocomplete="off"></textarea></label>
    <label class="feedback-check" hidden><input name="include" type="checkbox" checked> Include basic browser diagnostics</label>
    <p class="feedback-diagnostics-note" hidden>Only connection status, secure context, browser capability flags and a coarse window size are included. No logs or game data.</p>
    <button type="submit">Review public issue</button></form>
    <section hidden class="feedback-review"><h3>Review exactly what will be posted</h3><pre tabindex="0"></pre>
    <label class="feedback-check"><input type="checkbox" name="consent"> I reviewed this with an adult if needed, removed private information, and agree to post it publicly.</label>
    <div class="dialog-actions"><button type="button" class="feedback-edit">Edit</button><button type="button" class="feedback-send" disabled>Post public issue</button></div></section>
    <p class="feedback-status" role="status" aria-live="polite"></p>`;
  document.body.append(dialog);
  const form=dialog.querySelector('form'),review=dialog.querySelector('section'),status=dialog.querySelector('[role=status]');
  const close=dialog.querySelector('.close'),send=dialog.querySelector('.feedback-send'),edit=dialog.querySelector('.feedback-edit'),consent=review.querySelector('input');
  let report,available=false,pending=false,finished=false,uncertain=false;
  const uncertainMessage='We could not confirm whether GitHub received your report. Keep this preview and ask your adult admin to check the issues before sending it again.';
  const message=text=>{status.textContent=text;};
  button.onclick=async()=>{
    closeMenu();
    if(finished){form.reset();form.hidden=false;review.hidden=true;finished=false;report=null;edit.disabled=false;consent.disabled=false;updateKind();}
    dialog.showModal();
    if(uncertain){message(uncertainMessage);updateSend();return;}
    available=false;updateSend();message('Checking feedback availability…');
    try{available=(await api('/api/feedback')).available;message(available?'No GitHub account needed. Nothing is sent until you choose Post public issue.':'Feedback is not set up. Ask your adult admin to enable it. You can still prepare and review a draft.');}
    catch{available=false;message('Cannot connect to Infuser. You can still prepare a draft. Close and reopen to check again.');}
    updateSend();
  };
  const updateSend=()=>{send.disabled=!available||!consent.checked||pending||finished||uncertain;};
  function updateKind(){const bug=form.elements.kind.value==='bug';form.elements.include.closest('label').hidden=!bug;dialog.querySelector('.feedback-diagnostics-note').hidden=!bug;}
  form.elements.kind.onchange=updateKind;
  close.onclick=()=>{if(!pending)dialog.close();};
  dialog.addEventListener('cancel',event=>{if(pending)event.preventDefault();});
  dialog.addEventListener('close',()=>button.closest('details')?.querySelector('summary')?.focus());
  form.onsubmit=event=>{
    event.preventDefault();
    if(!form.elements.title.value.trim()||!form.elements.description.value.trim()){message('Please add a title and description.');return;}
    report={kind:form.elements.kind.value,title:form.elements.title.value.trim(),description:form.elements.description.value.trim(),diagnostics:null,public_consent:true};
    if(report.kind==='bug'&&form.elements.include.checked){
      report.diagnostics={online:navigator.onLine,secure_context:window.isSecureContext,webgl:typeof WebGLRenderingContext!=='undefined',webrtc:typeof RTCPeerConnection!=='undefined',webassembly:typeof WebAssembly!=='undefined',viewport:innerWidth<600?'small':innerWidth<1000?'medium':'large'};
    }
    const kind=report.kind==='bug'?'Bug':'Feature request';
    review.querySelector('pre').textContent=`[${kind}] ${report.title}\n\n${kind}\n\n${report.description}`+(report.diagnostics?'\n\nBrowser diagnostics (reviewed by sender):\n```json\n'+JSON.stringify(report.diagnostics,null,2)+'\n```':'');
    form.hidden=true;review.hidden=false;consent.checked=false;updateSend();review.querySelector('pre').focus();
  };
  consent.onchange=updateSend;
  edit.onclick=()=>{review.hidden=true;form.hidden=false;consent.checked=false;form.elements.title.focus();};
  send.onclick=async()=>{
    if(send.disabled)return;
    pending=true;updateSend();close.disabled=true;edit.disabled=true;consent.disabled=true;message('Posting to GitHub…');
    const controller=new AbortController(),timeout=setTimeout(()=>controller.abort(),15000);
    try{await api('/api/feedback',{method:'POST',body:JSON.stringify(report),signal:controller.signal});finished=true;message('Your public issue was posted. Thank you!');}
    catch(error){
      // A dropped response may follow successful creation. Never retry automatically.
      uncertain=!error.status||error.status>=500&&error.status!==503;
      message(uncertain?uncertainMessage:error.message);
      if(error.status===503)available=false;
    }finally{clearTimeout(timeout);pending=false;close.disabled=false;edit.disabled=finished||uncertain;consent.disabled=finished||uncertain;updateSend();}
  };
}
