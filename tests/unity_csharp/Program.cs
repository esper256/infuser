using System;
using System.Collections.Generic;
using System.IO;
using System.Threading.Tasks;
using Infuser.Unity;
using UnityEngine;
using UnityEditor;
using UnityEditor.PackageManager;
using UnityEditor.Build;
using UnityEditor.Build.Reporting;

internal static class Program
{
    [Serializable] public sealed class Progress { public int schema = 1; public int stars; }
    [Serializable] public sealed class Envelope { public string @event, json; }
    static void Check(bool condition, string message) { if (!condition) throw new Exception(message); }
    static async Task Fails(Func<Task> action) { try { await action(); } catch { return; } throw new Exception("Expected rejection"); }
    static string Json(object value) => JsonUtility.ToJson(value);
    static async Task Main()
    {
        var requests = new List<string>();
        var save = new JsonSave<Progress>((op, json) => {
            if (op == "save.load") return Task.FromResult(Json(new SaveSnapshot { exists = false }));
            requests.Add(json);
            if (requests.Count == 1) throw new IOException("Lost acknowledgement");
            return Task.FromResult(Json(new SaveResult { status = "saved", revision = "r1" }));
        }, "progress", "progress.json", value => { if (value.schema != 1 || value.stars < 0) throw new ArgumentException("Unsupported progress"); });
        await save.LoadAsync();
        await Fails(() => save.SaveAsync(new Progress(), new string('x', 81)));
        await Fails(() => save.SaveAsync(new Progress(), "line\nbreak"));
        await Fails(() => save.SaveAsync(new Progress { schema = 99 }));
        Check(!save.HasPending && requests.Count == 0, "Invalid input must not trap a pending checkpoint");
        var progress = new Progress { stars = 4 };
        await Fails(() => save.SaveAsync(progress, string.Concat(System.Linq.Enumerable.Repeat("🚀", 80))));
        Check(save.HasPending, "Unacknowledged checkpoint retained");
        progress.stars = 100;
        await Fails(() => save.SaveAsync(progress));
        await save.RetryAsync();
        Check(requests[0] == requests[1], "Retry must keep commit ID and exact checkpoint despite game mutations");
        Check(save.Value.stars == 4 && !save.HasPending, "Only acknowledged checkpoint becomes saved value");

        bool invalid = true; int commits = 0;
        var conflict = new JsonSave<Progress>((op, json) => {
            if (op == "save.load") return Task.FromResult(Json(new SaveSnapshot { exists = false }));
            if (op == "save.read") return Task.FromResult(Json(new SaveSnapshot { exists = true, revision = "candidate", json = Json(new Progress { schema = invalid ? 99 : 1, stars = 7 }), summary = "7 stars" }));
            commits++;
            if (commits == 1) return Task.FromResult(Json(new SaveResult { status = "conflict", current_revision = "other", candidate_revision = "candidate" }));
            Check(json.Contains("\"expectedRevision\":\"other\"") && json.Contains("\"candidateRevision\":\"candidate\""), "Resolve must name both revisions");
            return Task.FromResult(Json(new SaveResult { status = "saved", revision = "resolved" }));
        }, "progress", "progress.json", value => { if (value.schema != 1) throw new ArgumentException("Unsupported schema"); });
        await conflict.LoadAsync(); await conflict.SaveAsync(new Progress());
        await Fails(() => conflict.RetryAsync());
        await Fails(() => conflict.ResolveAsync(true));
        Check(conflict.HasConflict && commits == 1, "Invalid chosen save must preserve both conflict choices");
        invalid = false; await conflict.ResolveAsync(true);
        Check(!conflict.HasConflict && conflict.Value.stars == 7, "Validated conflict can be resolved");

        var delayed = new TaskCompletionSource<string>();
        var concurrent = new JsonSave<Progress>((op, json) => delayed.Task, "progress", "progress.json");
        var first = concurrent.LoadAsync();
        await Fails(() => concurrent.LoadAsync());
        delayed.SetResult(Json(new SaveSnapshot { exists = false })); await first;

        var client = new InfuserClient();
        var ordinary = InfuserClient.SessionArguments(null);
        Check(!ordinary.Contains("target") && !ordinary.Contains("intent"), "Ordinary Play omits absent targets");
        var resume = InfuserClient.SessionArguments(new SessionOptions { action = "resume", target = new SessionTarget { session_id = "observed" } });
        Check(!resume.Contains("player_id") && !resume.Contains("activity") && resume.Contains("observed"), "Resume omits optional empty player and activity");
        PeerMessage message = null; SocketMessage binary = null;
        client.PeerMessageReceived += value => message = value;
        client.SocketMessageReceived += value => binary = value;
        client.OnInfuserMessage(Json(new Envelope { @event = "peer_message", json = "{\"from\":\"host\",\"json\":\"{}\"}" }));
        client.OnInfuserMessage(Json(new Envelope { @event = "socket_message", json = "{\"base64\":\"AP8=\"}" }));
        Check(message.from == "host" && binary.IsBinary && binary.Bytes[1] == 255, "Typed callbacks retain their payloads");
        Check(!JsonUtility.FromJson<SocketMessage>("{\"text\":\"hello\",\"base64\":\"\",\"binary\":false}").IsBinary, "Empty absent string must not turn text into binary");
        Check(JsonUtility.FromJson<SocketMessage>("{\"base64\":\"\",\"binary\":true}").Bytes.Length == 0, "Empty binary frame stays binary");
#if UNITY_EDITOR
        Check(!(await client.ReadyAsync()).hosted, "Editor must stay standalone");
        await Fails(() => client.ConnectSessionAsync());
#endif
        var root = Path.Combine(Path.GetTempPath(), "infuser-unity-" + Guid.NewGuid().ToString("N"));
        try
        {
            var adapter = Path.Combine(root, "MovedAssets/Infuser");
            Directory.CreateDirectory(Path.Combine(adapter, "Editor")); Directory.CreateDirectory(Path.Combine(adapter, "Web"));
            File.WriteAllText(Path.Combine(adapter, "Web/bridge.mjs"), "bridge");
            AssetDatabase.ScriptPath = Path.Combine(adapter, "Editor/InfuserWebBuild.cs");
            Check(InfuserWebBuild.BridgeSource() == Path.Combine(adapter, "Web/bridge.mjs"), "Copied adapter can move");
            PackageInfo.Current = new PackageInfo { assetPath = "Packages/org.infuser.unity-web", resolvedPath = root };
            AssetDatabase.ScriptPath = "Packages/org.infuser.unity-web/MovedAssets/Infuser/Editor/InfuserWebBuild.cs";
            Check(InfuserWebBuild.BridgeSource() == Path.Combine(adapter, "Web/bridge.mjs"), "UPM cache path resolution");
            var export = Path.Combine(root, "export"); Directory.CreateDirectory(export);
            await Fails(() => { InfuserWebBuild.ValidateExport(export); return Task.CompletedTask; });
            File.WriteAllText(Path.Combine(export, "index.html"), "game");
            new InfuserWebBuild().OnPostprocessBuild(new BuildReport { summary = new BuildSummary { platform = BuildTarget.WebGL, outputPath = export } });
            Check(File.ReadAllText(Path.Combine(export, "infuser-unity.mjs")) == "bridge", "Hook copies actual bridge");
            InfuserWebBuild.ValidateExport(export);
        }
        finally { Directory.Delete(root, true); }
        Console.WriteLine("C# adapter/save/export checks passed (Unity test doubles; not Editor or IL2CPP acceptance).");
    }
}
