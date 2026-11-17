// Only explicit, bounded capability flags are collected. No logs or page data.
export function initFeedback(button, closeMenu, api, admin=false) {
  const dialog=document.createElement('dialog');dialog.id='feedback-dialog';dialog.setAttribute('aria-labelledby','feedback-title');
  dialog.innerHTML=`<button type="button" class="close" aria-label="Close feedback">×</button>
    <h2 id="feedback-title">Send feedback</h2><p class="feedback-intro"></p><p class="feedback-submitter" hidden></p>
    <p>Leave out names, email addresses, private game details, links, setup codes and passwords.</p>
    <section class="feedback-queue" hidden><p>Only your group’s admins can review these reports. Private reports expire after 30 days.</p><button type="button" class="feedback-refresh">Refresh queue</button><div class="feedback-entries"></div></section>
    <form><label>Type<select name="kind"><option value="feature">Feature request</option><option value="bug">Bug</option></select></label>
    <label>Title<input name="title" maxlength="80" required autocomplete="off"></label>
    <label>What would you like to tell us?<textarea name="description" maxlength="2000" rows="5" required autocomplete="off"></textarea></label>
    <label class="feedback-check" hidden><input name="include" type="checkbox" checked> Include basic browser diagnostics</label>
    <p class="feedback-diagnostics-note" hidden>Only online status, secure context, browser capability flags and a coarse window size. No logs or game data.</p>
    <button type="submit" class="feedback-preview">Review draft</button></form>
    <section hidden class="feedback-review"><h3>Review exactly what will be shared</h3><pre tabindex="0"></pre>
    <label class="feedback-check"><input type="checkbox" name="consent"><strong class="feedback-consent-text">I have reviewed the feedback</strong></label>
    <div class="dialog-actions"><button type="button" class="feedback-edit">Edit</button><button type="button" class="feedback-send" disabled>Send to admins</button></div></section>
    <p class="feedback-status" role="status" aria-live="polite"></p>`;
  document.body.append(dialog);
  const $=selector=>dialog.querySelector(selector), form=$('form'),review=$('.feedback-review'),queue=$('.feedback-queue');
  const close=$('.close'),send=$('.feedback-send'),edit=$('.feedback-edit'),consent=$('[name=consent]');
  let report,requestId,reviewId=null,snapshot=null,pending=false,finished=false,locked=false,available=false,closeTimer;
  const cancelAutoClose=()=>{clearTimeout(closeTimer);closeTimer=null;};
  const message=text=>{$('.feedback-status').textContent=text;};
  const updateSend=()=>{send.disabled=!consent.checked||pending||finished||locked||!!reviewId&&!available;};
  function updateKind(){const bug=form.elements.kind.value==='bug';form.elements.include.closest('label').hidden=!bug;$('.feedback-diagnostics-note').hidden=!bug;}
  function reset(){cancelAutoClose();form.reset();form.hidden=false;review.hidden=true;$('.feedback-submitter').hidden=true;report=null;requestId=null;snapshot=null;pending=false;finished=false;locked=false;consent.checked=false;consent.disabled=false;edit.disabled=false;updateKind();updateSend();}
  function copy(){
    $('.feedback-intro').textContent=reviewId?'Admin review: edit out private information. Only the report below will become a public GitHub issue when you explicitly approve it.':'Your feedback goes privately to your group’s admins. An admin can edit and approve it before anything is posted publicly on GitHub.';
    $('.feedback-preview').textContent=reviewId?'Review public issue':'Review draft';send.textContent=reviewId?'Post public issue':'Send to admins';
  }
  button.onclick=()=>{closeMenu();reviewId=null;queue.hidden=true;reset();copy();dialog.showModal();message('Nothing is sent to GitHub without admin approval.');};
  close.onclick=()=>{if(!pending)dialog.close();};
  dialog.addEventListener('cancel',event=>{if(pending)event.preventDefault();});
  dialog.addEventListener('close',()=>{cancelAutoClose();button.closest('details')?.querySelector('summary')?.focus();});
  form.elements.kind.onchange=updateKind;
  form.onsubmit=event=>{
    event.preventDefault();
    if(!form.elements.title.value.trim()||!form.elements.description.value.trim()){message('Please add a title and description.');return;}
    report={kind:form.elements.kind.value,title:form.elements.title.value.trim(),description:form.elements.description.value.trim(),diagnostics:null};
    if(report.kind==='bug'&&form.elements.include.checked){
      report.diagnostics=reviewId?snapshot:{online:navigator.onLine,secure_context:window.isSecureContext,webgl:typeof WebGLRenderingContext!=='undefined',webrtc:typeof RTCPeerConnection!=='undefined',webassembly:typeof WebAssembly!=='undefined',viewport:innerWidth<600?'small':innerWidth<1000?'medium':'large'};
    }
    requestId=crypto.randomUUID();
    const kind=report.kind==='bug'?'Bug':'Feature request';
    review.querySelector('pre').textContent=`[${kind}] ${report.title}\n\n${kind}\n\n${report.description}`+(report.diagnostics?'\n\nBrowser diagnostics (reviewed by sender):\n```json\n'+JSON.stringify(report.diagnostics,null,2)+'\n```':'');
    form.hidden=true;review.hidden=false;consent.checked=false;updateSend();review.querySelector('pre').focus();
  };
  consent.onchange=updateSend;
  edit.onclick=()=>{review.hidden=true;form.hidden=false;consent.checked=false;form.elements.title.focus();};
  send.onclick=async()=>{
    if(send.disabled)return;
    pending=true;updateSend();close.disabled=true;edit.disabled=true;consent.disabled=true;$('.feedback-refresh').disabled=true;message(reviewId?'Posting to GitHub…':'Sending privately to admins…');
    let intakeUncertain=false;
    const controller=new AbortController(),timeout=setTimeout(()=>controller.abort(),15000);
    try{
      const path=reviewId?'/api/feedback/review/'+encodeURIComponent(reviewId)+'/send':'/api/feedback';
      const body=reviewId?{report,public_consent:true}:{request_id:requestId,report};
      await api(path,{method:'POST',body:JSON.stringify(body),signal:controller.signal});finished=true;
      message(reviewId?'Your approved public issue was posted.':'Sent privately to your group’s admins. Nothing has been posted on GitHub.');
      closeTimer=setTimeout(()=>dialog.close(),3000);
    }catch(error){
      // Intake retries reuse the same ID and immutable report. Public posting is
      // durably claimed server-side; ambiguous results must never trigger retries.
      intakeUncertain=!reviewId&&(!error.status||error.status>=500);
      locked=!!reviewId&&(!error.status||error.status>=500&&error.status!==503||error.status===409);
      if(locked)message('Posting could not be confirmed. Refresh the queue and check GitHub before creating another report. This entry cannot be sent twice.');
      else if(!reviewId&&(!error.status||error.status>=500))message('Could not confirm receipt. You can safely retry Send to admins with this same draft.');
      else message(error.message);
      if(reviewId&&error.status===503)available=false;
    }finally{clearTimeout(timeout);pending=false;close.disabled=false;$('.feedback-refresh').disabled=false;edit.disabled=finished||locked||intakeUncertain;consent.disabled=finished||locked;updateSend();}
  };
  async function loadQueue(){
    if(pending)return;
    cancelAutoClose();
    form.hidden=true;review.hidden=true;reviewId=null;$('.feedback-submitter').hidden=true;$('.feedback-entries').replaceChildren();message('Loading private feedback…');
    try{
      const data=await api('/api/feedback/review');available=data.available;
      const list=$('.feedback-entries');list.replaceChildren();
      for(const entry of data.entries){
        const item=document.createElement('section'),title=document.createElement('h3'),state=document.createElement('p');
        const submitter=entry.submitter||'Unavailable profile';
        title.textContent=entry.report?.title||'Feedback';state.textContent=submitter+' · '+new Date(entry.created*1000).toLocaleDateString()+' · '+entry.state;item.append(title,state);
        if(entry.state==='pending'){
          const open=document.createElement('button');open.type='button';open.textContent='Review and edit';open.onclick=()=>{
            if(pending)return;
            reset();reviewId=entry.id;snapshot=entry.report.diagnostics;copy();
            $('.feedback-submitter').textContent='From '+submitter+' · Only visible to admins';$('.feedback-submitter').hidden=false;
            form.elements.kind.value=entry.report.kind;form.elements.title.value=entry.report.title;form.elements.description.value=entry.report.description;form.elements.include.checked=!!snapshot;updateKind();
            message(available?'Review the complete public issue before approving.':'GitHub posting is not configured. You can still review or dismiss feedback.');form.elements.title.focus();
          };item.append(open);
        }else{
          const note=document.createElement('p'),preview=document.createElement('pre');note.textContent='Already attempted. Check GitHub; this entry cannot be posted again.';preview.textContent=JSON.stringify(entry.report,null,2);item.append(note,preview);
        }
        const remove=document.createElement('button');remove.type='button';remove.textContent='Dismiss and delete text';remove.onclick=async()=>{
          if(pending)return;remove.disabled=true;
          try{await api('/api/feedback/review/'+encodeURIComponent(entry.id),{method:'DELETE'});await loadQueue();}
          catch(error){message(error.message);remove.disabled=false;}
        };item.append(remove);list.append(item);
      }
      message(data.entries.length?(available?'Choose a report to review.':'GitHub posting is not configured. Private review is still available.'):'No feedback awaiting review.');
    }catch(error){$('.feedback-entries').replaceChildren();message(error.message);}
  }
  $('.feedback-refresh').onclick=loadQueue;
  if(!admin)return null;
  const reviewButton=document.createElement('button');reviewButton.type='button';reviewButton.textContent='Review feedback';reviewButton.setAttribute('aria-haspopup','dialog');
  reviewButton.onclick=async()=>{closeMenu();queue.hidden=false;reset();form.hidden=true;dialog.showModal();$('.feedback-intro').textContent='Private feedback from your group. Review and edit before approving a public GitHub issue.';await loadQueue();};
  return reviewButton;
}
