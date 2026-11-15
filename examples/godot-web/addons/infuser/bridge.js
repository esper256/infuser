// Experimental Godot web adapter. Only HTTP/session/save plumbing lives here;
// Godot owns WebRTCMultiplayerPeer, RPCs, and all game state.
(() => {
  const sdk = import('/_infuser/sdk/v1.js');
  window.InfuserGodot = {
    async call(id, operation, encoded, callback) {
      try {
        const args = JSON.parse(encoded);
        const api = await sdk;
        let value;
        if (operation === 'ready') value = await api.ready;
        else if (operation === 'load') value = await api.cloud.load(args.group);
        else if (operation === 'commit') value = await api.cloud.commit(args.group, args.checkpoint);
        else if (operation === 'presence') value = await api.presence.get();
        else if (operation === 'peers') {
          if (!/^\/(?:[a-f0-9]+(?:\/(?:signals|ice))?)?(?:\?after=\d+)?$/.test(args.path)) throw Error('Invalid signaling path');
          const response = await fetch('/_infuser/api/v1/peers' + args.path.replace(/^\/$/, ''), {
            method: args.method, credentials: 'same-origin', signal: AbortSignal.timeout(5000),
            ...(args.body == null ? {} : {headers: {'Content-Type':'application/json'}, body: JSON.stringify(args.body)})
          });
          value = await response.json();
          if (!response.ok) throw Object.assign(Error(value.error || 'Session unavailable'), {status:response.status});
        } else throw Error('Unknown Infuser operation');
        callback(id, JSON.stringify({ok:true, value}));
      } catch (error) {
        callback(id, JSON.stringify({ok:false, error:error.message, status:error.status || 0}));
      }
    },
    uuid() { return crypto.randomUUID(); },
    options() { return JSON.stringify(Object.fromEntries(new URLSearchParams(location.search))); },
    network(status) { if (parent !== window) parent.postMessage({type:'infuser-network', status}, location.origin); }
  };
})();
