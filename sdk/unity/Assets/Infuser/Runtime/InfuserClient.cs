using System;
using System.Collections;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading.Tasks;
using UnityEngine;
using UnityEngine.Scripting;

namespace Infuser.Unity
{
    public sealed class InfuserException : Exception
    {
        public string Code { get; }
        public int Status { get; }
        public InfuserException(string message, string code, int status = 0) : base(message) { Code = code; Status = status; }
    }
    [Serializable] public sealed class Identity
    {
        public bool hosted, guest;
        public string app_id, build_id, player_id, name, library_url;
    }
    [Serializable] public sealed class LaunchIntent { public string kind, session_id, player_id; }
    [Serializable] public sealed class LaunchRequest { public LaunchIntent intent; }
    [Serializable] public sealed class PeerOptions
    {
        public bool relayOnly, joinable;
        public string sessionId;
        public LaunchIntent intent;
    }
    [Serializable] public sealed class PeerRoom { public string id, hostId, role, epoch; }
    [Serializable] public sealed class PeerMessage { public string from, json; }
    [Serializable] public sealed class SendResult { public bool sent; }

    // One persistent receiver per game instance. Call from Unity's main thread.
    public sealed class InfuserClient : MonoBehaviour
    {
        private static InfuserClient instance;
        private readonly Dictionary<string, TaskCompletionSource<string>> pending = new Dictionary<string, TaskCompletionSource<string>>();
        private readonly Dictionary<string, Coroutine> deadlines = new Dictionary<string, Coroutine>();
        public event Action<string, string> Event;
        public event Action<PeerState> PeerStateChanged;
        public event Action<PeerMessage> PeerMessageReceived;
        public event Action<NetworkEnd> PeerEnded;
        public event Action<SocketMessage> SocketMessageReceived;
        public event Action<NetworkEnd> SocketEnded;
        public event Action<NetworkError> NetworkFailed;
        public static InfuserClient Instance
        {
            get
            {
                if (instance == null)
                {
                    var go = new GameObject("Infuser-" + Guid.NewGuid().ToString("N"));
                    instance = go.AddComponent<InfuserClient>();
                    DontDestroyOnLoad(go);
                }
                return instance;
            }
        }
        [Serializable] private sealed class Reply { public string id, json, error, code, @event; public bool ok; public int status; }
        [Serializable] private sealed class PeerSend { public string to, json; }
        [Serializable] private sealed class JoinFields { public bool relayOnly, joinable; public string sessionId; }
        [Serializable] private sealed class Joinability { public bool joinable; }
        [Serializable] private sealed class SessionFields { public string action; public int timeoutMs; }
        [Serializable] private sealed class ActivityField { public string activity; }
        [Serializable] private sealed class SessionIdField { public string session_id; }
        [Serializable] private sealed class TextMessage { public string text; }
        [Serializable] private sealed class BinaryMessage { public string base64; }
#if UNITY_WEBGL && !UNITY_EDITOR
        [DllImport("__Internal")] private static extern void Infuser_Call(string receiver, string id, string op, string json);
        [DllImport("__Internal")] private static extern void Infuser_Dispose();
#endif
        public Task<Identity> ReadyAsync() => CallAsync<Identity>("ready");
        public async Task<LaunchRequest> ConsumeLaunchIntentAsync()
        {
            var launch = await CallAsync<LaunchRequest>("launch_intent");
            // Inline serializable classes can materialize with default fields.
            // Ordinary Play must still have no target after Unity deserialization.
            if (launch.intent != null && string.IsNullOrEmpty(launch.intent.kind)) launch.intent = null;
            return launch;
        }
        public Task<string> PresenceAsync() => CallJsonAsync("presence");
        public async Task<PresenceSnapshot> GetPresenceAsync()
        {
            var value = await CallAsync<PresenceSnapshot>("presence");
            if (value.session != null && string.IsNullOrEmpty(value.session.id)) value.session = null;
            return value;
        }
        public Task<PeerRoom> JoinPeersAsync(PeerOptions options)
        {
            if (options == null) throw new ArgumentNullException(nameof(options));
            // JsonUtility may serialize a null inline class as default fields.
            // Omit an absent target entirely so ordinary Play stays untargeted.
            var json = JsonUtility.ToJson(new JoinFields { relayOnly = options.relayOnly, joinable = options.joinable, sessionId = options.sessionId });
            if (options.intent != null) json = json.Substring(0, json.Length - 1) + ",\"intent\":" + JsonUtility.ToJson(options.intent) + "}";
            return CallAsync<PeerRoom>("peer.join", json);
        }
        public Task<SendResult> SendPeerAsync<T>(string to, T message) => CallAsync<SendResult>("peer.send", JsonUtility.ToJson(new PeerSend { to = to, json = JsonUtility.ToJson(message) }));
        public Task<string> BroadcastAsync<T>(T message) => CallJsonAsync("peer.broadcast", JsonUtility.ToJson(new PeerSend { json = JsonUtility.ToJson(message) }));
        public Task<string> SetJoinableAsync(bool value) => CallJsonAsync("peer.joinable", JsonUtility.ToJson(new Joinability { joinable = value }));
        public Task<string> LeaveMultiplayerAsync() => CallJsonAsync("network.close");
        public Task<SessionConnection> ConnectSessionAsync(SessionOptions options = null)
            => CallAsync<SessionConnection>("socket.connect", SessionArguments(options));
        internal static string SessionArguments(SessionOptions options)
        {
            options = options ?? new SessionOptions();
            var action = string.IsNullOrEmpty(options.action) ? "play" : options.action;
            var json = JsonUtility.ToJson(new SessionFields { action = action, timeoutMs = options.timeoutMs });
            if (action == "play" || action == "create")
                json = json.Substring(0, json.Length - 1) + "," + JsonUtility.ToJson(new ActivityField { activity = string.IsNullOrEmpty(options.activity) ? "multiplayer" : options.activity }).Substring(1);
            // Omit absent nested values, including for ordinary Play in Unity.
            if (options.intent != null) json = json.Substring(0, json.Length - 1) + ",\"intent\":" + JsonUtility.ToJson(options.intent) + "}";
            else if (options.target != null)
            {
                var target = string.IsNullOrEmpty(options.target.player_id)
                    ? JsonUtility.ToJson(new SessionIdField { session_id = options.target.session_id })
                    : JsonUtility.ToJson(options.target);
                json = json.Substring(0, json.Length - 1) + ",\"target\":" + target + "}";
            }
            return json;
        }
        public Task<SendResult> SendTextAsync(string text)
        {
            if (text == null) throw new ArgumentNullException(nameof(text));
            return CallAsync<SendResult>("socket.send", JsonUtility.ToJson(new TextMessage { text = text }));
        }
        public Task<SendResult> SendBytesAsync(byte[] bytes)
        {
            if (bytes == null) throw new ArgumentNullException(nameof(bytes));
            return CallAsync<SendResult>("socket.send", JsonUtility.ToJson(new BinaryMessage { base64 = Convert.ToBase64String(bytes) }));
        }

        // JSON escape hatch for multi-file/binary checkpoints and dedicated
        // sockets. It is an adapter API, not a replacement HTTP contract.
        public async Task<T> CallAsync<T>(string operation, string arguments = "{}") => JsonUtility.FromJson<T>(await CallJsonAsync(operation, arguments));
        public Task<string> CallJsonAsync(string operation, string arguments = "{}")
        {
#if UNITY_WEBGL && !UNITY_EDITOR
            var id = Guid.NewGuid().ToString("N");
            var completion = new TaskCompletionSource<string>();
            pending.Add(id, completion);
            deadlines.Add(id, StartCoroutine(Expire(id)));
            try { Infuser_Call(gameObject.name, id, operation, arguments); }
            catch (Exception error) { pending.Remove(id); StopCoroutine(deadlines[id]); deadlines.Remove(id); completion.TrySetException(error); }
            return completion.Task;
#else
            if (operation == "ready") return Task.FromResult("{\"hosted\":false}");
            return Task.FromException<string>(new InfuserException("Use the game's standalone path outside Infuser Web.", "not_hosted"));
#endif
        }
        private IEnumerator Expire(string id)
        {
            yield return new WaitForSecondsRealtime(120);
            deadlines.Remove(id);
            if (pending.TryGetValue(id, out var completion))
            {
                pending.Remove(id);
                completion.TrySetException(new InfuserException("Response not confirmed. Keep pending saves; do not report success.", "timeout"));
            }
        }
        [Preserve] public void OnInfuserMessage(string payload)
        {
            var reply = JsonUtility.FromJson<Reply>(payload);
            if (!string.IsNullOrEmpty(reply.@event))
            {
                switch (reply.@event)
                {
                    case "peer_state": PeerStateChanged?.Invoke(JsonUtility.FromJson<PeerState>(reply.json)); break;
                    case "peer_message": PeerMessageReceived?.Invoke(JsonUtility.FromJson<PeerMessage>(reply.json)); break;
                    case "peer_end": PeerEnded?.Invoke(JsonUtility.FromJson<NetworkEnd>(reply.json)); break;
                    case "socket_message": SocketMessageReceived?.Invoke(JsonUtility.FromJson<SocketMessage>(reply.json)); break;
                    case "socket_end": SocketEnded?.Invoke(JsonUtility.FromJson<NetworkEnd>(reply.json)); break;
                    case "socket_error": case "session_error": NetworkFailed?.Invoke(JsonUtility.FromJson<NetworkError>(reply.json)); break;
                }
                Event?.Invoke(reply.@event, reply.json); return;
            }
            if (reply.id == null || !pending.TryGetValue(reply.id, out var completion)) return;
            pending.Remove(reply.id);
            if (deadlines.TryGetValue(reply.id, out var deadline)) { StopCoroutine(deadline); deadlines.Remove(reply.id); }
            if (reply.ok) completion.TrySetResult(reply.json);
            else completion.TrySetException(new InfuserException(reply.error, reply.code, reply.status));
        }
        private void OnDestroy()
        {
#if UNITY_WEBGL && !UNITY_EDITOR
            Infuser_Dispose();
#endif
            foreach (var completion in pending.Values) completion.TrySetCanceled();
            pending.Clear();
            deadlines.Clear();
            if (instance == this) instance = null;
        }
    }
}
