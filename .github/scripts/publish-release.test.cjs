const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { prepare, publish, serverFiles, macFiles } = require('./publish-release.cjs');
const previous = 'a'.repeat(40), commit = 'b'.repeat(40), newer = 'c'.repeat(40);

function fixture(t, channel = 'server', options = {}) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'infuser-release-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  for (const file of channel === 'server' ? serverFiles : macFiles) fs.writeFileSync(path.join(directory, file), `built ${commit}: ${file}`);
  if (channel === 'server' && !options.noChangelog) {
    fs.writeFileSync(path.join(directory, 'changelog.json'), JSON.stringify({
      version: 1, since: previous, changes: [{ text: 'Games remember where you left off', audience: 'family' }],
    }));
  }
  const writes = [], reads = [], uploads = [];
  const tag = `${channel === 'server' ? 'build' : 'macos'}-${commit}`;
  let draft = options.existing || null, latestReads = 0;
  const request = async (route, method = 'GET', body) => {
    if (method !== 'GET') {
      writes.push({ route, method, body });
      if (method === 'POST') return draft = { id: 123, ...body };
      if (method === 'PATCH') return { ...draft, ...body };
      throw new Error('Unexpected write');
    }
    reads.push(route);
    if (route.endsWith('/releases/latest')) {
      if (options.apiError) throw new Error('HTTP 503');
      latestReads++;
      return options.noLatest ? null : { tag_name: `build-${options.race && latestReads > 1 ? newer : options.latest || previous}` };
    }
    if (route.endsWith(`/compare/${commit}...main`)) return { status: options.offMain ? 'diverged' : 'ahead' };
    if (route.includes('/compare/')) return { status: route.includes(`${newer}...`) ? 'behind' : route.includes(`${commit}...${commit}`) ? 'identical' : options.diverged ? 'diverged' : 'ahead' };
    if (route.endsWith(`/releases/tags/${tag}`)) return draft;
    if (route.endsWith(`/git/ref/tags/${tag}`)) return options.wrongTag ? { object: { type: 'commit', sha: newer } } : null;
    if (route.endsWith('/releases/123')) return { ...draft, assets: options.badAssets ? [] : uploads.at(-1).map(file => ({ name: path.basename(file), size: fs.statSync(file).size, state: 'uploaded', digest: options.badDigest ? 'sha256:bad' : 'sha256:' + crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') })) };
    throw new Error('Unexpected read: ' + route);
  };
  const upload = async args => {
    assert.deepEqual(args.slice(0, 3), ['release', 'upload', tag]);
    if (options.uploadFailure) throw new Error('Upload failed');
    uploads.push(args.slice(3, args.indexOf('--repo')));
  };
  const run = () => publish({ directory, commit, channel, repository: 'esper256/infuser', runUrl: 'https://github.com/esper256/infuser/actions/runs/123' }, request, upload);
  return { directory, writes, reads, uploads, run, tag };
}

test('server release is complete, single-commit, and retains both existing installer aliases', t => {
  const f = fixture(t), files = prepare(f.directory, commit, 'server');
  const read = name => JSON.parse(fs.readFileSync(path.join(f.directory, name)));
  assert.equal(read('release.json').commit, commit);
  assert.equal(read('release.json').host, undefined);
  assert.deepEqual(Object.keys(read('release.json').artifacts), ['linux-amd64', 'linux-arm64']);
  assert.equal(read('play-release.json').commit, commit);
  assert.equal(read('play-release.json').protocol, 1);
  for (const manifest of ['release.json', 'play-release.json']) for (const artifact of Object.values(read(manifest).artifacts)) {
    assert.equal(artifact.sha256, crypto.createHash('sha256').update(fs.readFileSync(path.join(f.directory, artifact.file))).digest('hex'));
  }
  assert.equal(fs.readFileSync(path.join(f.directory, 'Infuser-Setup.exe'), 'utf8'), fs.readFileSync(path.join(f.directory, 'Infuser-Play-Setup.exe'), 'utf8'));
  assert.equal(fs.readFileSync(path.join(f.directory, 'Infuser.flatpak'), 'utf8'), fs.readFileSync(path.join(f.directory, 'Infuser-Play.flatpak'), 'utf8'));
  const sums = fs.readFileSync(path.join(f.directory, 'SHA256SUMS'), 'utf8').trim().split('\n');
  assert.equal(sums.length, files.length - 1);
  for (const line of sums) {
    const [hash, file] = line.split('  ');
    assert.equal(hash, crypto.createHash('sha256').update(fs.readFileSync(path.join(f.directory, file))).digest('hex'));
  }
  assert(!files.some(file => file.includes('macos')));
});
test('changelog ships beside the manifest and in the release notes, not as a release.json field', async t => {
  const f = fixture(t);
  const changelog = {
    version: 1,
    since: previous,
    changes: [
      { text: 'Games remember where you left off', audience: 'family', pr: 12 },
      { text: 'Cache the release build', audience: 'internal' },
    ],
  };
  fs.writeFileSync(path.join(f.directory, 'changelog.json'), JSON.stringify(changelog));
  assert.equal(await f.run(), 'published');
  const manifest = JSON.parse(fs.readFileSync(path.join(f.directory, 'release.json')));
  assert.equal(manifest.changelog, undefined);
  assert.equal(manifest.host, undefined);
  assert.equal(f.uploads[0].length, 12);
  assert.ok(f.uploads[0].some(file => file.endsWith('changelog.json')));
  assert.match(f.writes[0].body.body, /## What's new\n- Games remember where you left off/);
  assert.match(f.writes[0].body.body, /## Behind the scenes\n- Cache the release build/);
  assert.doesNotMatch(f.writes[0].body.body.split('Tested ')[0], /[0-9a-f]{40}/);
  const sums = fs.readFileSync(path.join(f.directory, 'SHA256SUMS'), 'utf8');
  assert.match(sums, /changelog\.json/);
});
test('a release can name the host step it cannot take effect without', t => {
  const f = fixture(t);
  prepare(f.directory, commit, 'server', ['reboot', 'reboot']);
  const manifest = JSON.parse(fs.readFileSync(path.join(f.directory, 'release.json')));
  assert.deepEqual(manifest.host, ['reboot']);
  assert.throws(() => prepare(f.directory, commit, 'server', ['kernel']));
});
test('Mac manifest requires both architectures and never includes server/Play assets', t => {
  const f = fixture(t, 'macos'), files = prepare(f.directory, commit, 'macos');
  assert.deepEqual(files, [...macFiles, 'release.json', 'SHA256SUMS']);
  assert.deepEqual(Object.keys(JSON.parse(fs.readFileSync(path.join(f.directory, 'release.json'))).artifacts), ['macos-amd64', 'macos-arm64']);
});
for (const channel of ['server', 'macos']) for (const damage of ['missing', 'empty', 'symlink']) test(`${channel}: ${damage} artifact prevents all publication writes`, async t => {
  const f = fixture(t, channel), target = path.join(f.directory, channel === 'server' ? serverFiles[0] : macFiles[0]);
  fs.unlinkSync(target);
  if (damage === 'empty') fs.writeFileSync(target, '');
  if (damage === 'symlink') fs.symlinkSync(path.join(f.directory, channel === 'server' ? serverFiles[1] : macFiles[1]), target);
  await assert.rejects(f.run()); assert.equal(f.writes.length, 0); assert.equal(f.uploads.length, 0);
});
test('a tested ancestor can publish even when main has advanced', async t => {
  const f = fixture(t); assert.equal(await f.run(), 'published');
  assert.equal(f.writes[0].body.target_commitish, commit);
  assert.equal(f.writes[0].body.draft, true);
  assert.deepEqual(f.writes.at(-1).body, { draft: false, prerelease: false, make_latest: 'true' });
  assert.equal(f.uploads[0].length, 12);
});
test('a server release without changelog.json publishes nothing', async t => {
  const f = fixture(t, 'server', { noChangelog: true });
  await assert.rejects(f.run(), /Server release is missing changelog\.json/);
  assert.equal(f.writes.length, 0);
  assert.equal(f.uploads.length, 0);
  assert.equal(fs.existsSync(path.join(f.directory, 'release.json')), false);
});
test('a Mac release does not require changelog.json', async t => {
  const f = fixture(t, 'macos');
  assert.equal(fs.existsSync(path.join(f.directory, 'changelog.json')), false);
  assert.equal(await f.run(), 'published');
  assert.equal(f.uploads[0].some(file => file.endsWith('changelog.json')), false);
});
test('initial server release works without a previous latest', async t => {
  const f = fixture(t, 'server', { noLatest: true }); assert.equal(await f.run(), 'published');
});
for (const latest of [commit, newer]) test(`equal/newer latest (${latest[0]}) prevents rollback and redundant uploads`, async t => {
  const f = fixture(t, 'server', { latest }); assert.equal(await f.run(), 'superseded');
  assert.equal(f.writes.length, 0); assert.equal(f.uploads.length, 0);
});
for (const option of ['offMain', 'diverged', 'wrongTag', 'apiError']) test(`${option} fails closed`, async t => {
  const f = fixture(t, 'server', { [option]: true }); await assert.rejects(f.run());
  assert.equal(f.writes.length, 0); assert.equal(f.uploads.length, 0);
});
for (const option of ['badAssets', 'badDigest', 'uploadFailure']) test(`${option} leaves draft unpublished`, async t => {
  const f = fixture(t, 'server', { [option]: true }); await assert.rejects(f.run());
  assert.equal(f.writes.filter(w => w.method === 'PATCH').length, 0);
});
test('a newer release appearing during upload cannot be replaced', async t => {
  const f = fixture(t, 'server', { race: true }); assert.equal(await f.run(), 'superseded-draft');
  assert.equal(f.writes.filter(w => w.method === 'PATCH').length, 0);
});
test('Mac track never queries or changes latest, even for an older commit', async t => {
  const f = fixture(t, 'macos', { latest: newer }); assert.equal(await f.run(), 'published');
  assert(!f.reads.some(route => route.endsWith('/latest')));
  assert.equal(f.writes[0].body.tag_name, `macos-${commit}`);
  assert.deepEqual(f.writes.at(-1).body, { draft: false, prerelease: true, make_latest: 'false' });
});
test('published Mac release is immutable and not uploaded again', async t => {
  const f = fixture(t, 'macos', { existing: { id: 123, tag_name: `macos-${commit}`, draft: false } });
  assert.equal(await f.run(), 'already-published'); assert.equal(f.writes.length, 0); assert.equal(f.uploads.length, 0);
});
test('an interrupted draft can safely resume', async t => {
  const f = fixture(t, 'server', { existing: { id: 123, tag_name: `build-${commit}`, target_commitish: commit, draft: true } });
  assert.equal(await f.run(), 'published'); assert.equal(f.writes.filter(w => w.method === 'POST').length, 0);
});
test('invalid commits, channels and repositories cannot publish', async t => {
  const f = fixture(t);
  assert.throws(() => prepare(f.directory, 'main', 'server'));
  assert.throws(() => prepare(f.directory, commit, 'other'));
  await assert.rejects(publish({ directory: f.directory, commit, channel: 'server', repository: 'other/repo' }));
});
test('workflow boundaries keep server release independent and Mac source tied to the selected published commit', () => {
  const server = fs.readFileSync(path.join(__dirname, '../workflows/release.yml'), 'utf8');
  const mac = fs.readFileSync(path.join(__dirname, '../workflows/macos-publisher.yml'), 'utf8');
  const releaseSteps = server.slice(server.indexOf('  release:'));
  assert(releaseSteps.indexOf('uses: actions/checkout') < releaseSteps.indexOf('uses: actions/download-artifact'), 'checkout must not erase downloaded artifacts');
  assert.match(server, /needs: \[build, desktop\]/); assert.doesNotMatch(server, /macos-publisher:/);
  const buildUpload = server.slice(server.indexOf('name: infuser-linux-'), server.indexOf('desktop:'));
  assert.match(buildUpload, /dist\/infuser-linux-\$\{\{ matrix\.arch \}\}/);
  assert.match(buildUpload, /dist\/changelog\.json/);
  assert.match(releaseSteps, /merge-multiple: true/);
  assert.match(releaseSteps, /test -f dist\/changelog\.json/);
  assert.match(releaseSteps, /test ! -e dist\/dist\/changelog\.json/);
  for (const workflow of [server, mac]) {
    assert.match(workflow, /cancel-in-progress: false/);
    assert.doesNotMatch(workflow, /cancel-in-progress: true|allow-unsafe-pr-checkout|pull_request_target/);
  }
  assert.doesNotMatch(mac, /workflow_run:/);
  assert.match(mac, /cron: '23 6 \* \* \*'/);
  assert.match(mac, /github.ref == 'refs\/heads\/main'/);
  assert.match(mac, /INFUSER_BUILD_SHA: \$\{\{ needs.plan.outputs.commit \}\}/);
  assert.match(mac, /ref: \$\{\{ env.INFUSER_BUILD_SHA \}\}/);
  assert.match(mac, /if: needs.plan.outputs.build == 'true'/);
  assert.match(mac, /needs: \[plan, macos-publisher\]/);
});

test('a draft targeting another commit is never reused', async t => {
  const f = fixture(t, 'server', { existing: { id: 123, tag_name: `build-${commit}`, target_commitish: newer, draft: true } });
  await assert.rejects(f.run(), /Unexpected draft/); assert.equal(f.uploads.length, 0);
});
