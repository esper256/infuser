// Report only that a player interacted. Never send the key, position or button.
export function watchInput(doc, report) {
  let last = -Infinity;
  function active() {
    const now = performance.now();
    if (doc.hidden || now-last < 15000) return;
    last = now;
    report().catch(()=>{}); // Presence is optional during a network interruption.
  }
  for (const type of ['pointerdown','pointermove','keydown','wheel']) {
    doc.addEventListener(type, event=>{if(event.isTrusted)active();}, {capture:true,passive:true});
  }
  return active;
}
