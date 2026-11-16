using System;
using System.Threading.Tasks;
using Infuser.Unity;
using UnityEngine;

// Integration fixture, not a production game. No external assets or packages.
public sealed class BeaconGarden : MonoBehaviour
{
    [Serializable] public sealed class Progress { public int schema = 1; public int stars; }
    [Serializable] private sealed class Packet { public string type; public int pulse; }
    private InfuserClient client;
    private JsonSave<Progress> save;
    private Progress progress;
    private Identity identity;
    private PeerRoom room;
    private bool busy = true, relayOnly;
    private int pulse;
    private string status = "Connecting…", cloudChoice, deviceChoice;
    private Transform beacon;
    private const string LocalKey = "beacon-garden-standalone";

    private async void Start()
    {
        var camera = new GameObject("Camera").AddComponent<Camera>();
        camera.transform.position = new Vector3(0, 3, -7);
        camera.transform.LookAt(Vector3.zero);
        camera.clearFlags = CameraClearFlags.SolidColor;
        camera.backgroundColor = new Color(0.025f, 0.04f, 0.1f);
        var light = new GameObject("Light").AddComponent<Light>();
        light.type = LightType.Directional; light.transform.rotation = Quaternion.Euler(40, -30, 0);
        beacon = GameObject.CreatePrimitive(PrimitiveType.Cube).transform;
        client = InfuserClient.Instance;
        client.Event += OnEvent;
        await Run(async () =>
        {
            identity = await client.ReadyAsync();
            if (identity.hosted)
            {
                save = new JsonSave<Progress>(client, "progress", "progress.json");
                await save.LoadAsync();
                progress = save.Value ?? new Progress(); Validate(progress);
                var launch = await client.ConsumeLaunchIntentAsync();
                if (launch.intent != null) await Join(launch.intent);
            }
            else
            {
                progress = JsonUtility.FromJson<Progress>(PlayerPrefs.GetString(LocalKey, "{\"schema\":1,\"stars\":0}"));
                Validate(progress);
            }
            status = identity.hosted ? (identity.guest ? "Guest — fresh progress" : "Ready, " + identity.name) : "Standalone — local progress";
        });
    }
    private static void Validate(Progress value)
    {
        if (value == null || value.schema != 1 || value.stars < 0) throw new Exception("Unsupported save. Keep the original and return to the Library.");
    }
    private void Update()
    {
        if (beacon != null) { beacon.Rotate(0, 30 * Time.deltaTime, 0); beacon.localScale = Vector3.one * (1 + 0.12f * Mathf.Sin(Time.time * 2 + pulse)); }
    }
    private async Task Join(LaunchIntent intent = null)
    {
        room = await client.JoinPeersAsync(new PeerOptions { relayOnly = relayOnly, joinable = false, intent = intent });
        if (room.role == "host") await client.SetJoinableAsync(true);
        status = "Multiplayer: " + room.role;
    }
    private async void OnEvent(string name, string json)
    {
        try
        {
            if (name == "peer_message")
            {
                var message = JsonUtility.FromJson<PeerMessage>(json);
                var packet = JsonUtility.FromJson<Packet>(message.json);
                if (room == null || packet == null) return;
                if (room.role == "host" && packet.type == "tap") { pulse++; await Snapshot(); }
                else if (room.role != "host" && message.from == room.hostId && packet.type == "pulse" && packet.pulse >= 0) pulse = packet.pulse;
            }
            else if (name == "peer_state" && room?.role == "host") await Snapshot();
            else if (name == "peer_end" || name == "session_error") { room = null; status = "Session ended — return to the title screen or Library."; }
        }
        catch (Exception error) { status = error.Message; }
    }
    private Task<string> Snapshot() => client.BroadcastAsync(new Packet { type = "pulse", pulse = pulse });
    private async Task Collect()
    {
        progress.stars++;
        if (room?.role == "host") { pulse++; await Snapshot(); }
        else if (room != null) await client.SendPeerAsync(room.hostId, new Packet { type = "tap" });
        status = "Collected — save at a checkpoint.";
    }
    private async Task ShowResult(SaveResult result)
    {
        if (result.status == "saved") { progress = save.Value; status = "Saved"; cloudChoice = deviceChoice = null; }
        else
        {
            var cloud = await save.ReadConflictAsync(false); var device = await save.ReadConflictAsync(true);
            Validate(JsonUtility.FromJson<Progress>(cloud.json)); Validate(JsonUtility.FromJson<Progress>(device.json));
            cloudChoice = cloud.summary; deviceChoice = device.summary;
            status = "Two progress copies — choose which to continue.";
        }
    }
    private async Task Save()
    {
        if (!identity.hosted) { PlayerPrefs.SetString(LocalKey, JsonUtility.ToJson(progress)); PlayerPrefs.Save(); status = "Saved locally"; return; }
        await ShowResult(save.HasPending ? await save.RetryAsync() : await save.SaveAsync(progress, progress.stars + " stars"));
    }
    private async Task Run(Func<Task> action)
    {
        busy = true;
        try { await action(); } catch (Exception error) { status = error.Message; }
        finally { busy = false; }
    }
    private void OnGUI()
    {
        GUILayout.BeginArea(new Rect(20, 20, 420, Screen.height - 40), GUI.skin.box);
        GUILayout.Label("Beacon Garden · Unity integration preview");
        GUILayout.Label(status);
        GUILayout.Label("Stars: " + (progress?.stars ?? 0) + " · Shared pulse: " + pulse);
        GUI.enabled = !busy && progress != null;
        if (save?.HasConflict == true)
        {
            if (GUILayout.Button("Cloud: " + cloudChoice)) _ = Run(async () => await ShowResult(await save.ResolveAsync(false)));
            if (GUILayout.Button("This session: " + deviceChoice)) _ = Run(async () => await ShowResult(await save.ResolveAsync(true)));
        }
        else
        {
            if (save?.HasPending != true && GUILayout.Button("Collect a star")) _ = Run(Collect);
            if (GUILayout.Button(save?.HasPending == true ? "Retry save" : "Save progress")) _ = Run(Save);
        }
        if (identity?.hosted == true)
        {
            relayOnly = GUILayout.Toggle(relayOnly, "Force relay (test)");
            if (room == null && GUILayout.Button("Join multiplayer")) _ = Run(() => Join());
            if (room != null && GUILayout.Button("Leave multiplayer")) _ = Run(async () => { await client.LeaveMultiplayerAsync(); room = null; });
        }
        GUI.enabled = true;
        GUILayout.EndArea();
    }
    private void OnDestroy() { if (client != null) client.Event -= OnEvent; }
}
