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
        private readonly InfuserClient client;
        private readonly string group, file;
        private string revision;
        private Write pending;
        private SaveResult conflict;
        private bool loaded, busy;
        public T Value { get; private set; }
        public bool HasPending => pending != null;
        public bool HasConflict => conflict != null;
        public JsonSave(InfuserClient client, string group, string file) { this.client = client; this.group = group; this.file = file; }
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
                var snapshot = await client.CallAsync<SaveSnapshot>("save.load", JsonUtility.ToJson(new Read { group = group, file = file }));
                var value = snapshot.exists ? JsonUtility.FromJson<T>(snapshot.json) : null;
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
                pending = new Write { group = group, file = file, expectedRevision = revision,
                    commitId = Guid.NewGuid().ToString("N"), json = JsonUtility.ToJson(value), summary = summary };
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
            return client.CallAsync<SaveSnapshot>("save.read", JsonUtility.ToJson(new Read { group = group, file = file,
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
            var result = await client.CallAsync<SaveResult>("save.commit", JsonUtility.ToJson(pending));
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
