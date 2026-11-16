// Keep this file ES5-compatible for Unity's Emscripten library parser.
mergeInto(LibraryManager.library, {
  $InfuserUnityPlugin: { loading: null, bridge: null, disposed: false },
  Infuser_Call__deps: ['$InfuserUnityPlugin'],
  Infuser_Call: function (receiverPtr, idPtr, opPtr, jsonPtr) {
    var receiver = UTF8ToString(receiverPtr), id = UTF8ToString(idPtr);
    var op = UTF8ToString(opPtr), json = UTF8ToString(jsonPtr);
    if (!InfuserUnityPlugin.loading) {
      InfuserUnityPlugin.loading = new Promise(function (resolve, reject) {
        var script = document.createElement('script');
        script.type = 'module';
        script.src = new URL('infuser-unity.mjs', document.baseURI).href;
        script.onload = function () { resolve(); };
        script.onerror = function () { reject(new Error('Could not load infuser-unity.mjs. Rebuild with the Infuser Web build hook.')); };
        document.head.appendChild(script);
      }).then(function () {
        if (InfuserUnityPlugin.disposed) throw new Error('Unity instance stopped.');
        InfuserUnityPlugin.bridge = globalThis.InfuserUnity.createBridge(function (message) {
          SendMessage(receiver, 'OnInfuserMessage', JSON.stringify(message));
        });
        return InfuserUnityPlugin.bridge;
      });
    }
    InfuserUnityPlugin.loading.then(function (bridge) { return bridge.call(id, op, json); }).catch(function (error) {
      if (InfuserUnityPlugin.disposed) return;
      SendMessage(receiver, 'OnInfuserMessage', JSON.stringify({id:id, ok:false, error:error.message, code:'bridge_unavailable', status:0}));
    });
  },
  Infuser_Dispose__deps: ['$InfuserUnityPlugin'],
  Infuser_Dispose: function () {
    InfuserUnityPlugin.disposed = true;
    if (InfuserUnityPlugin.bridge) InfuserUnityPlugin.bridge.dispose();
  }
});
