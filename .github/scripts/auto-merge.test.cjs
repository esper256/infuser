const { test } = require('node:test');
const assert = require('node:assert/strict');
const { eligible, checksPass, run } = require('./auto-merge.cjs');
const now = Date.parse('2026-01-01T00:10:00Z');
const repository = { full_name: 'esper256/infuser' };
const pr = () => ({
  number: 42, state: 'open', draft: false, merged: false,
  user: { id: 528155, login: 'esper256' },
  head: { sha: 'head', repo: repository },
  base: { sha: 'base', ref: 'main', repo: repository },
  merge_commit_sha: 'merge', mergeable: true, mergeable_state: 'clean',
  updated_at: '2026-01-01T00:00:00Z',
});
const success = { status: 'completed', conclusion: 'success' };
function fixture({ first = pr(), fresh = first, checks = {}, statuses = {}, base = 'base', mergeError, dispatchError } = {}) {
  const writes = [], reads = [], messages = [];
  let gets = 0;
  const github = { rest: {
    pulls: {
      list() {},
      async get() { return { data: ++gets === 1 ? first : fresh }; },
      async merge(args) {
        writes.push(['merge', args]);
        if (mergeError) throw mergeError;
        return { data: { merged: true, sha: 'merged-sha' } };
      },
    },
    checks: { listForRef() {} },
    repos: {
      listCommitStatusesForRef() {},
      async getBranch() { return { data: { commit: { sha: base } } }; },
    },
    actions: { async createWorkflowDispatch(args) {
      writes.push(['dispatch', args]);
      if (dispatchError) throw dispatchError;
    } },
  }, async paginate(method, args) {
    reads.push(args);
    if (method === github.rest.pulls.list) return [first];
    if (method === github.rest.checks.listForRef) return checks[args.ref] || [];
    if (method === github.rest.repos.listCommitStatusesForRef) return statuses[args.ref] || [];
    throw new Error('Unexpected API');
  } };
  return { writes, reads, messages, args: { github, context: { repo: { owner: 'esper256', repo: 'infuser' } },
    core: { info: m => messages.push(m) }, now: () => now, dryRun: false } };
}

test('owner ready PR is eligible, including unstable for inspection of actual checks', () => {
  assert.equal(eligible(pr(), now), true);
  assert.equal(eligible({ ...pr(), mergeable_state: 'unstable' }, now), true);
});
for (const [name, change] of Object.entries({
  draft: { draft: true }, closed: { state: 'closed' }, merged: { merged: true },
  outsider: { user: { id: 7, login: 'outsider' } },
  spoofedLogin: { user: { id: 7, login: 'esper256' } },
  fork: { head: { sha: 'head', repo: { full_name: 'other/infuser' } } },
  deletedFork: { head: { sha: 'head', repo: null } },
  wrongBase: { base: { ...pr().base, ref: 'other' } },
  conflict: { mergeable: false }, unknown: { mergeable: null },
  behind: { mergeable_state: 'behind' }, blocked: { mergeable_state: 'blocked' },
  recent: { updated_at: '2026-01-01T00:09:30Z' }, invalidTime: { updated_at: null },
})) {
  test(`skip ${name}`, async () => {
    const f = fixture({ first: { ...pr(), ...change } });
    await run(f.args);
    assert.deepEqual(f.writes, []);
  });
}
test('empty checks are permitted; neutral and skipped count as completed', () => {
  assert.equal(checksPass([], []), true);
  assert.equal(checksPass([success, { ...success, conclusion: 'neutral' }, { ...success, conclusion: 'skipped' }], []), true);
});
for (const conclusion of ['failure', 'cancelled', 'timed_out', 'action_required', 'stale', null]) {
  test(`reject check conclusion ${conclusion}`, () => {
    assert.equal(checksPass([{ ...success, conclusion }], []), false);
  });
}
test('queued and running checks block', () => {
  for (const status of ['queued', 'in_progress', 'waiting', 'pending']) {
    assert.equal(checksPass([{ ...success, status }], []), false);
  }
});
test('legacy statuses use newest context result across all pages', () => {
  assert.equal(checksPass([], [{ id: 2, context: 'ci', state: 'success' }, { id: 1, context: 'ci', state: 'failure' }]), true);
  assert.equal(checksPass([], [{ id: 2, context: 'ci', state: 'success' }, { id: 3, context: 'ci', state: 'pending' }]), false);
  assert.equal(checksPass([], [{ id: 2, context: 'ci', state: 'success' }, { id: 1, context: 'lint', state: 'error' }]), false);
});
for (const ref of ['head', 'merge']) {
  test(`failure on ${ref} prevents merge`, async () => {
    const f = fixture({ checks: { [ref]: [{ ...success, conclusion: 'failure' }] } });
    await run(f.args);
    assert.deepEqual(f.writes, []);
  });
  test(`pending status on ${ref} prevents merge`, async () => {
    const f = fixture({ statuses: { [ref]: [{ id: 1, context: 'ci', state: 'pending' }] } });
    await run(f.args);
    assert.deepEqual(f.writes, []);
  });
}
for (const [name, change] of Object.entries({
  draft: { draft: true }, closed: { state: 'closed' },
  head: { head: { ...pr().head, sha: 'new' } },
  base: { base: { ...pr().base, sha: 'new' } },
  merge: { merge_commit_sha: 'new' },
})) {
  test(`recheck detects changed ${name}`, async () => {
    const f = fixture({ fresh: { ...pr(), ...change } });
    await run(f.args);
    assert.deepEqual(f.writes, []);
  });
}
test('current main must still match the checked base', async () => {
  const f = fixture({ base: 'new' }); await run(f.args); assert.deepEqual(f.writes, []);
});
test('dry run never writes and is default', async () => {
  const f = fixture(); delete f.args.dryRun;
  await run(f.args); assert.deepEqual(f.writes, []);
  assert.match(f.messages.at(-1), /WOULD merge/);
});
test('happy path uses expected SHA then dispatches release; both refs paginated', async () => {
  const f = fixture({ checks: { head: [success], merge: [success] } });
  await run(f.args);
  assert.deepEqual(f.writes.map(w => w[0]), ['merge', 'dispatch']);
  assert.equal(f.writes[0][1].sha, 'head');
  assert.equal(f.writes[0][1].merge_method, 'merge');
  assert.deepEqual(f.writes[1][1], { owner: 'esper256', repo: 'infuser', workflow_id: 'release.yml', ref: 'main' });
  assert.deepEqual(f.reads.filter(r => r.filter === 'latest').map(r => r.ref), ['head', 'merge']);
  assert.ok(f.reads.every(r => r.per_page === 100));
});
test('server rejects stale head or conflict: no release dispatch', async () => {
  const f = fixture({ mergeError: new Error('409') });
  await assert.rejects(run(f.args), /409/); assert.equal(f.writes.length, 1);
});
test('failed release dispatch surfaces error after merge, without another merge', async () => {
  const f = fixture({ dispatchError: new Error('403') });
  await assert.rejects(run(f.args), /403/); assert.equal(f.writes.length, 2);
});
test('API failure fails closed', async () => {
  const f = fixture(); f.args.github.paginate = async () => { throw new Error('API unavailable'); };
  await assert.rejects(run(f.args), /API unavailable/); assert.deepEqual(f.writes, []);
});
test('unexpected repository fails closed', async () => {
  const f = fixture(); f.args.context.repo.owner = 'other';
  await assert.rejects(run(f.args), /Unexpected repository/); assert.deepEqual(f.writes, []);
});
test('missing synthetic merge commit waits', async () => {
  const f = fixture({ first: { ...pr(), merge_commit_sha: null } });
  await run(f.args); assert.deepEqual(f.writes, []);
});
test('merge result false stops without release', async () => {
  const f = fixture();
  f.args.github.rest.pulls.merge = async () => ({ data: { merged: false, message: 'blocked' } });
  await assert.rejects(run(f.args), /merge rejected/); assert.deepEqual(f.writes, []);
});
test('stops after one successful merge even with multiple candidates', async () => {
  const f = fixture(); const paginate = f.args.github.paginate;
  f.args.github.paginate = (method, args) => method === f.args.github.rest.pulls.list
    ? [pr(), { ...pr(), number: 43 }] : paginate(method, args);
  await run(f.args); assert.deepEqual(f.writes.map(w => w[0]), ['merge', 'dispatch']);
});
test('draft candidate does not starve a later ready PR', async () => {
  const f = fixture(); const paginate = f.args.github.paginate;
  f.args.github.paginate = (method, args) => method === f.args.github.rest.pulls.list
    ? [{ ...pr(), number: 41 }, pr()] : paginate(method, args);
  f.args.github.rest.pulls.get = async ({ pull_number }) => ({ data: pull_number === 41
    ? { ...pr(), number: 41, draft: true } : pr() });
  await run(f.args); assert.equal(f.writes[0][1].pull_number, 42);
});
