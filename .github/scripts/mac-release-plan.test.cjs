const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { plan, relevant, publishedMac, gitAdapter } = require('./mac-release-plan.cjs');
const old = 'a'.repeat(40), head = 'b'.repeat(40), unrelated = 'c'.repeat(40);
const mac = sha => ({ tag_name: `macos-${sha}`, target_commitish: sha, published_at: '2026-01-01', prerelease: true, assets: ['infuser-macos-amd64', 'infuser-macos-arm64', 'release.json', 'SHA256SUMS'].map(name => ({ name, state: 'uploaded', size: 1, digest: 'sha256:' + 'f'.repeat(64) })) });
const input = () => ({ event: 'schedule', latest: { tag_name: `build-${head}`, published_at: '2026-01-02' }, releases: [mac(old)] });
const git = paths => ({ ancestor: () => true, history: () => [head, old], changed: () => paths });
test('source, shared dependencies, SDK and journey inputs trigger; UI and docs do not', () => {
  for (const file of ['src/publisher.rs', 'src/server.rs', 'native-protocol/src/lib.rs', 'sdk/unity/a', 'examples/a', '.cargo/config.toml', 'Cargo.toml', 'Cargo.lock', 'build.rs', 'rust-toolchain.toml', 'tests/publisher_journey.rs', 'tests/support/packager.rs', 'tests/fixtures/a', '.github/workflows/macos-publisher.yml', '.github/scripts/publish-release.cjs', '.github/scripts/mac-release-plan.test.cjs']) assert(relevant(file), file);
  for (const file of ['docs/ci.md', 'README.md', 'web/chat.js', 'desktop/src/main.rs', 'tests/other.rs', '.github/workflows/pr-checks.yml']) assert(!relevant(file), file);
});
test('daily batches all relevant changes since last published Mac ancestor', () => {
  assert.equal(plan(input(), git(['docs/a', 'src/publisher.rs'])).build, true);
  assert.equal(plan(input(), git(['docs/a', 'web/chat.js'])).build, false);
  assert.equal(plan(input(), git([])).build, false);
});
test('manual bypasses diff but never rebuilds an immutable exact release', () => {
  const data = input(); data.event = 'workflow_dispatch';
  assert.equal(plan(data, git([])).build, true);
  data.releases.push(mac(head));
  assert.equal(plan(data, git(['src/a'])).build, false);
  data.event = 'schedule'; assert.equal(plan(data, git(['src/a'])).build, false);
});
test('no published ancestor builds; unrelated release dates do not choose baseline', () => {
  const data = input(); data.releases = [mac(unrelated)];
  assert.equal(plan(data, git([])).build, true);
  data.releases.push(mac(old));
  assert.equal(plan(data, git([])).baseline, old);
});
test('closest ancestor wins regardless of API order', () => {
  const data = input(); data.releases.push(mac(unrelated));
  assert.equal(plan(data, { ...git([]), history: () => [head, unrelated, old] }).baseline, unrelated);
});
test('draft, partial, wrong-target, and unverified Mac releases cannot suppress retries', () => {
  for (const change of [r => r.draft = true, r => delete r.published_at, r => r.target_commitish = 'main', r => r.assets.pop(), r => r.assets[0].size = 0, r => r.assets[0].digest = null, r => r.assets[0].state = 'new']) {
    const release = mac(head); change(release);
    assert.equal(publishedMac(release), null);
    const data = input(); data.releases = [release];
    assert.equal(plan(data, git([])).build, true);
  }
});
test('rejects unsupported events, malformed/unpublished server candidates and off-main commits', () => {
  assert.throws(() => plan({ ...input(), event: 'pull_request' }, git([])));
  for (const latest of [null, {}, { tag_name: 'build-main' }, { ...input().latest, draft: true }, { ...input().latest, prerelease: true }, { tag_name: `build-${head}` }]) assert.throws(() => plan({ ...input(), latest }, git([])));
  assert.throws(() => plan(input(), { ...git([]), ancestor: () => false }));
});
test('real git handles deletion, renames, unusual paths and main ancestry', t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mac-plan-'));
  const previous = process.cwd();
  t.after(() => { process.chdir(previous); fs.rmSync(dir, { recursive: true, force: true }); });
  process.chdir(dir);
  const run = (...args) => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  run('init'); run('config', 'user.name', 'Test'); run('config', 'user.email', 'test@example.invalid');
  fs.mkdirSync('src'); fs.writeFileSync('src/old.rs', 'old'); fs.writeFileSync('src/delete.rs', 'deleted');
  run('add', '.'); run('commit', '-m', 'base'); const base = run('rev-parse', 'HEAD');
  fs.renameSync('src/old.rs', 'README.md'); fs.unlinkSync('src/delete.rs'); fs.writeFileSync('src/odd\nname.rs', 'new');
  run('add', '.'); run('commit', '-m', 'changes'); const candidate = run('rev-parse', 'HEAD'); run('update-ref', 'refs/remotes/origin/main', candidate);
  const adapter = gitAdapter();
  assert(adapter.ancestor(base)); assert.deepEqual(adapter.history(candidate), [candidate, base]);
  assert.deepEqual(adapter.changed(base, candidate).sort(), ['README.md', 'src/delete.rs', 'src/odd\nname.rs', 'src/old.rs'].sort());
  assert.throws(() => adapter.ancestor('invalid'));
});
