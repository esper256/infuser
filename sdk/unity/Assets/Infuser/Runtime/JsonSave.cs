using System;
using System.Threading.Tasks;
using UnityEngine;

namespace Infuser.Unity
{
    [Serializable] public sealed class SaveSnapshot
    {
        public string revision, json, summary;
        public bool exists;
        public long created_at;
    }
    [Serializable] public sealed class SaveResult
    {
        public string status, revision, current_revision, candidate_revision;
    }
    // For one declared JSON file per group. T must be a JsonUtility-compatible
    // [Serializable] class. Call Load before gameplay; validate its schema.
    public sealed class JsonSave<T> where T : class
    {
        [Serializable] private sealed class Read { public string group, file, revision; }
        [Serializable] private sealed class Write
        {
            public string group, file, expectedRevision, commitId, json, summary, candidateRevision;
        }
        private readonly Func<string, string, Task<string>> call;
        private readonly Action<T> validate;
        private readonly string group, file;
        private string revision;
        private Write pending;
        private SaveResult conflict;
        private bool loaded, busy;
        public T Value { get; private set; }
        public bool HasPending => pending != null;
        public bool HasConflict => conflict != null;
        public JsonSave(InfuserClient client, string group, string file, Action<T> validate = null)
            : this(client.CallJsonAsync, group, file, validate) { }
        internal JsonSave(Func<string, string, Task<string>> call, string group, string file, Action<T> validate = null)
        {
            this.call = call ?? throw new ArgumentNullException(nameof(call));
            if (string.IsNullOrWhiteSpace(group) || string.IsNullOrWhiteSpace(file))
                throw new ArgumentException("Use the group and file declared in the game manifest.");
            this.group = group; this.file = file; this.validate = validate;
        }
        private async Task<R> Call<R>(string operation, string json) => JsonUtility.FromJson<R>(await call(operation, json));
        private T Decode(string json)
        {
            var value = JsonUtility.FromJson<T>(json);
            if (value == null) throw new ArgumentException("A checkpoint must contain a JSON object.");
            validate?.Invoke(value);
            return value;
        }
        private static void ValidateSummary(string summary)
        {
            int length = 0;
            for (int i = 0; i < (summary ?? "").Length; i++, length++)
            {
                if (char.IsControl(summary[i])) throw new ArgumentException("Save summaries cannot contain control characters.", nameof(summary));
                if (char.IsHighSurrogate(summary[i]) && i + 1 < summary.Length && char.IsLowSurrogate(summary[i + 1])) i++;
            }
            if (length > 80) throw new ArgumentException("Save summaries must be at most 80 characters.", nameof(summary));
        }
        private void Enter()
        {
            if (busy) throw new InvalidOperationException("Wait for the current save operation.");
            busy = true;
        }
        public async Task<SaveSnapshot> LoadAsync()
        {
            if (HasPending) throw new InvalidOperationException("Retry or resolve the pending checkpoint first.");
            Enter();
            try
            {
                var snapshot = await Call<SaveSnapshot>("save.load", JsonUtility.ToJson(new Read { group = group, file = file }));
                var value = snapshot.exists ? Decode(snapshot.json) : null;
                revision = snapshot.revision; Value = value; loaded = true;
                return snapshot;
            }
            finally { busy = false; }
        }
        public async Task<SaveResult> SaveAsync(T value, string summary = "")
        {
            if (!loaded || HasPending) throw new InvalidOperationException("Load first; retry or resolve any pending save before starting another.");
            if (value == null) throw new ArgumentNullException(nameof(value));
            Enter();
            try
            {
                ValidateSummary(summary);
                var json = JsonUtility.ToJson(value);
                Decode(json); // Validate the serialized snapshot before retaining it for retry.
                pending = new Write { group = group, file = file, expectedRevision = revision,
                    commitId = Guid.NewGuid().ToString("N"), json = json, summary = summary ?? "" };
                return await Commit();
            }
            finally { busy = false; }
        }
        public async Task<SaveResult> RetryAsync()
        {
            if (pending == null || conflict != null) throw new InvalidOperationException("No retryable save. Resolve conflicts explicitly.");
            Enter();
            try { return await Commit(); } finally { busy = false; }
        }
        public Task<SaveSnapshot> ReadConflictAsync(bool device)
        {
            if (conflict == null) throw new InvalidOperationException("No save conflict.");
            return Call<SaveSnapshot>("save.read", JsonUtility.ToJson(new Read { group = group, file = file,
                revision = device ? conflict.candidate_revision : conflict.current_revision }));
        }
        // Show both copies and validate the chosen schema before calling this.
        // A concurrent write can produce another conflict; it is never overwritten.
        public async Task<SaveResult> ResolveAsync(bool useDevice)
        {
            if (conflict == null) throw new InvalidOperationException("No save conflict.");
            Enter();
            try
            {
                var chosen = await ReadConflictAsync(useDevice);
                Decode(chosen.json);
                ValidateSummary(chosen.summary);
                pending = new Write { group = group, file = file, expectedRevision = conflict.current_revision,
                    candidateRevision = conflict.candidate_revision, commitId = Guid.NewGuid().ToString("N"),
                    json = chosen.json, summary = chosen.summary };
                conflict = null;
                return await Commit();
            }
            finally { busy = false; }
        }
        private async Task<SaveResult> Commit()
        {
            var result = await Call<SaveResult>("save.commit", JsonUtility.ToJson(pending));
            if (result.status == "saved")
            {
                revision = result.revision; Value = JsonUtility.FromJson<T>(pending.json); pending = null; conflict = null;
            }
            else if (result.status == "conflict") conflict = result;
            else throw new InfuserException("Save not confirmed; keep the pending checkpoint.", "invalid_response");
            return result;
        }
    }
}
