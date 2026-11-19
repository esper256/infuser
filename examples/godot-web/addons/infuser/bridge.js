// Experimental Godot web adapter. Only HTTP/session/save plumbing lives here;
// Godot owns WebRTCMultiplayerPeer, RPCs, and all game state.
(() => {
  const sdk = import('/_infuser/sdk/v1.js');
  // performance.now alone may stop during OS sleep. Fence leases with both
  // clocks, never allowing a backwards wall-clock adjustment to extend them.
  let wall = Date.now(), monotonic = performance.now();
  window.InfuserGodot = {
    async call(id, operation, encoded, callback) {
      try {
        const args = JSON.parse(encoded);
        const api = await sdk;
        let value;
        if (operation === 'ready') value = await api.ready;
        else if (operation === 'load') value = await api.cloud.load(args.group);
        else if (operation === 'commit') value = await api.cloud.commit(args.group, args.checkpoint);
        else if (operation === 'read') value = await api.cloud.read(args.group, args.revision);
        else if (operation === 'resolve') value = await api.cloud.resolve(args.group, args.checkpoint);
        else if (operation === 'launch_intent') value = await api.launchIntent.consume();
        else if (operation === 'set_activity') value = await api.presence.setActivity(args.activity);
        else if (operation === 'presence') value = await api.presence.get();
        else if (operation === 'peers') {
          if (!/^\/(?:[a-f0-9]+(?:\/(?:signals|ice|joinability))?)?(?:\?after=\d+)?$/.test(args.path)) throw Error('Invalid signaling path');
          const response = await fetch('/_infuser/api/v1/peers' + args.path.replace(/^\/$/, ''), {
            method: args.method, credentials: 'same-origin', signal: AbortSignal.timeout(5000),
            ...(args.body == null ? {} : {headers: {'Content-Type':'application/json'}, body: JSON.stringify(args.body)})
          });
          value = await response.json();
          if (!response.ok) throw Object.assign(Error(value.error || 'Session unavailable'), {status:response.status});
        } else throw Error('Unknown Infuser operation');
        callback(id, JSON.stringify({ok:true, value}));
      } catch (error) {
        callback(id, JSON.stringify({ok:false, error:error.message, status:error.status || 0, code:error.code || ''}));
      }
    },
    uuid() { return crypto.randomUUID(); },
    now() { const next=performance.now();wall=Math.max(Date.now(),wall+next-monotonic);monotonic=next;return wall; },
    startTicker(callback) { return setInterval(()=>callback(),1000); },
    stopTicker(id) { clearInterval(id); },
    options() { return JSON.stringify(Object.fromEntries(new URLSearchParams(location.search))); },
    network(status) { if (parent !== window) parent.postMessage({type:'infuser-network', status}, location.origin); }
  };
})();
