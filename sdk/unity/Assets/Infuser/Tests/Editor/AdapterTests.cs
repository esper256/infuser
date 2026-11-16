using System;
using System.IO;
using System.Threading.Tasks;
using Infuser.Unity;
using NUnit.Framework;
using UnityEngine;

public sealed class AdapterTests
{
    [Serializable] public sealed class Progress { public int stars; }
    [Test] public void BridgeIsAvailableInInstalledLayout() => Assert.That(File.Exists(InfuserWebBuild.BridgeSource()), Is.True);
    [Test] public async Task EditorIsStandalone()
    {
        Assert.That((await InfuserClient.Instance.ReadyAsync()).hosted, Is.False);
    }
    [Test] public async Task SaveRetryKeepsRealUnitySerializedCheckpoint()
    {
        string first = null; int writes = 0;
        var save = new JsonSave<Progress>((operation, json) => {
            if (operation == "save.load") return Task.FromResult("{\"exists\":false,\"revision\":null}");
            writes++;
            if (writes == 1) { first = json; throw new IOException("Lost acknowledgement"); }
            Assert.That(json, Is.EqualTo(first));
            return Task.FromResult("{\"status\":\"saved\",\"revision\":\"saved\"}");
        }, "progress", "progress.json");
        await save.LoadAsync();
        var progress = new Progress { stars = 4 };
        try { await save.SaveAsync(progress); Assert.Fail("Expected lost acknowledgement"); } catch (IOException) { }
        progress.stars = 8;
        await save.RetryAsync();
        Assert.That(save.Value.stars, Is.EqualTo(4));
        Assert.That(save.HasPending, Is.False);
    }
    [Test] public void PayloadsUseUnityFields()
    {
        var data = JsonUtility.FromJson<SocketMessage>("{\"base64\":\"AP8=\"}");
        Assert.That(data.Bytes, Is.EqualTo(new byte[] { 0, 255 }));
        Assert.That(JsonUtility.FromJson<PeerMessage>("{\"from\":\"host\",\"json\":\"{}\"}").from, Is.EqualTo("host"));
        Assert.That(JsonUtility.FromJson<SocketMessage>("{\"text\":\"hello\",\"binary\":false}").IsBinary, Is.False);
        Assert.That(JsonUtility.FromJson<SocketMessage>("{\"base64\":\"\",\"binary\":true}").Bytes, Is.Empty);
        var resume = InfuserClient.SessionArguments(new SessionOptions { action = "resume", target = new SessionTarget { session_id = "observed" } });
        Assert.That(resume, Does.Not.Contain("player_id"));
        Assert.That(resume, Does.Not.Contain("activity"));
        Assert.That(InfuserClient.SessionArguments(null), Does.Not.Contain("target"));
    }
}
