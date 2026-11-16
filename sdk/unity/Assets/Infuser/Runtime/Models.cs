using System;

namespace Infuser.Unity
{
    [Serializable] public sealed class PresencePlayer
    {
        public string player_id, name;
        public bool guest, is_self, online, game_open, multiplayer;
    }
    [Serializable] public sealed class GameSession
    {
        public string id, label, activity, build_id, host_player_id, reason;
        public bool joinable;
        public int player_count, max_players;
        public PresencePlayer[] players;
    }
    [Serializable] public sealed class PresenceSnapshot
    {
        public string app_id, build_id, self_id, unavailable_reason;
        public bool can_start;
        public PresencePlayer[] players;
        public GameSession session;
        public GameSession[] sessions;
    }
    [Serializable] public sealed class PeerPlayer { public string id, player_id, name; public bool guest; }
    [Serializable] public sealed class PeerState
    {
        public string self_id, host_id, role, epoch, build_id;
        public int lease_ms, max_players;
        public bool accepting_players;
        public PeerPlayer[] peers;
        public string[] connected;
    }
    [Serializable] public sealed class SessionTarget { public string session_id, player_id; }
    [Serializable] public sealed class SessionOptions
    {
        public string action = "play", activity = "multiplayer";
        public SessionTarget target;
        public LaunchIntent intent;
        public int timeoutMs = 60000;
    }
    [Serializable] public sealed class SessionConnection
    {
        public string attempt_id, lease_id;
        public GameSession session;
    }
    [Serializable] public sealed class SocketMessage
    {
        public string text, base64;
        public bool binary;
        public bool IsBinary => binary || !string.IsNullOrEmpty(base64);
        public byte[] Bytes => IsBinary ? Convert.FromBase64String(base64 ?? "") : null;
    }
    [Serializable] public sealed class NetworkEnd { public string reason; public int code; }
    [Serializable] public sealed class NetworkError { public string error, code; public int status; }
}
