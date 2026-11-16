// Publication is serialized by the workflow. Never mix commits or modify a published release.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');
const SHA = /^[0-9a-f]{40}$/;
const serverFiles = ['infuser-linux-amd64', 'infuser-linux-arm64',
  'infuser-play-linux-x86_64', 'infuser-play-windows-x86_64.exe',
  'Infuser-Play-Setup.exe', 'Infuser-Play.flatpak'];
const macFiles = ['infuser-macos-amd64', 'infuser-macos-arm64'];

function prepare(directory, commit, channel) {
  if (!SHA.test(commit)) throw new Error('Invalid release commit');
  if (!['server', 'macos'].includes(channel)) throw new Error('Invalid release channel');
  const files = [...(channel === 'server' ? serverFiles : macFiles)];
  const digest = file => {
    const target = path.join(directory, file), stat = fs.lstatSync(target);
    if (!stat.isFile() || stat.size === 0) throw new Error(`Missing or empty regular artifact: ${file}`);
    return crypto.createHash('sha256').update(fs.readFileSync(target)).digest('hex');
  };
  // Validate the complete set before writing manifests or making any API writes.
  const hashes = Object.fromEntries(files.map(file => [file, digest(file)]));
  const artifact = file => ({ file, sha256: hashes[file] });
  const write = (file, data) => { fs.writeFileSync(path.join(directory, file), JSON.stringify(data, null, 2) + '\n'); files.push(file); };
  const platforms = channel === 'server' ? ['linux-amd64', 'linux-arm64'] : ['macos-amd64', 'macos-arm64'];
  write('release.json', { commit, artifacts: Object.fromEntries(platforms.map(p => [p, artifact('infuser-' + p)])) });
  if (channel === 'server') {
    write('play-release.json', { protocol: 1, commit, artifacts: {
      'linux-x86_64': artifact('infuser-play-linux-x86_64'),
      'windows-x86_64': artifact('infuser-play-windows-x86_64.exe'),
    } });
    for (const [source, alias] of [['Infuser-Play-Setup.exe', 'Infuser-Setup.exe'], ['Infuser-Play.flatpak', 'Infuser.flatpak']]) {
      fs.copyFileSync(path.join(directory, source), path.join(directory, alias)); files.push(alias);
    }
  }
  fs.writeFileSync(path.join(directory, 'SHA256SUMS'), files.map(file => `${digest(file)}  ${file}\n`).join(''));
  return [...files, 'SHA256SUMS'];
}

function cli(args, input) {
  return execFileSync('gh', args, { input, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] });
}
function api(route, method = 'GET', body) {
  try {
    return JSON.parse(cli(['api', route, '--method', method, ...(body ? ['--input', '-'] : [])], body && JSON.stringify(body)));
  } catch (error) {
    if (method === 'GET' && /\(HTTP 404\)/.test(String(error.stderr))) return null;
    throw error;
  }
}

async function publish({ directory, commit, channel, repository, runUrl }, request = api, upload = cli) {
  if (repository !== 'esper256/infuser') throw new Error('Unexpected repository');
  const files = prepare(directory, commit, channel);
  const root = `repos/${repository}`, tag = `${channel === 'server' ? 'build' : 'macos'}-${commit}`;
  const compare = async (base, head) => {
    const result = await request(`${root}/compare/${base}...${head}`);
    if (!result || !['ahead', 'behind', 'identical', 'diverged'].includes(result.status)) throw new Error('Cannot establish commit ancestry');
    return result.status;
  };
  if (!['ahead', 'identical'].includes(await compare(commit, 'main'))) throw new Error('Candidate is not an ancestor of main');
  // Even an old manual rerun must not roll automatic updaters back.
  const canPromote = async () => {
    const latest = await request(`${root}/releases/latest`);
    if (!latest) return true;
    const match = /^build-([0-9a-f]{40})$/.exec(latest.tag_name);
    if (!match) throw new Error('Unrecognized latest release; refusing to replace it');
    const relation = await compare(match[1], commit);
    if (relation === 'diverged') throw new Error('Latest release has diverged from candidate');
    return relation === 'ahead';
  };
  if (channel === 'server' && !await canPromote()) return 'superseded';
  let existing = await request(`${root}/releases/tags/${tag}`);
  // Refuse a pre-existing tag pointing elsewhere, including annotated tags.
  let ref = await request(`${root}/git/ref/tags/${tag}`);
  for (let depth = 0; ref?.object?.type === 'tag' && depth < 5; depth++) ref = await request(`${root}/git/tags/${ref.object.sha}`);
  if (ref && (ref.object?.type !== 'commit' || ref.object.sha !== commit)) throw new Error('Release tag does not match the tested commit');
  if (existing && !existing.draft) return 'already-published';
  if (!existing) existing = await request(`${root}/releases`, 'POST', {
    tag_name: tag, target_commitish: commit, name: `${channel === 'server' ? 'Server and Play' : 'Mac publisher'} ${commit.slice(0, 8)}`,
    body: `Tested ${channel} artifacts from commit ${commit}.\n\n${runUrl}\n\n${channel === 'macos' ? 'Independent Mac publisher track; the prerelease flag keeps this out of server/Play automatic updates. Verify downloads against SHA256SUMS from this tag.' : 'Linux servers and Infuser Play. Mac publishers are published separately as macos-<commit> releases.'}`,
    draft: true, prerelease: channel === 'macos', make_latest: 'false',
  });
  if (!existing?.id || existing.tag_name !== tag || !existing.draft || existing.target_commitish !== commit) throw new Error('Unexpected draft release response');
  await upload(['release', 'upload', tag, ...files.map(file => path.join(directory, file)), '--repo', repository, '--clobber']);
  const uploaded = await request(`${root}/releases/${existing.id}`);
  const expected = new Map(files.map(file => [file, {
    size: fs.statSync(path.join(directory, file)).size,
    digest: 'sha256:' + crypto.createHash('sha256').update(fs.readFileSync(path.join(directory, file))).digest('hex'),
  }]));
  if (!uploaded?.draft || uploaded.target_commitish !== commit || uploaded.tag_name !== tag || uploaded.assets?.length !== expected.size ||
      !uploaded.assets.every(asset => expected.get(asset.name)?.size === asset.size &&
        expected.get(asset.name)?.digest === asset.digest && asset.state === 'uploaded')) {
    throw new Error('Draft assets are incomplete or unexpected; leaving unpublished');
  }
  if (!['ahead', 'identical'].includes(await compare(commit, 'main'))) throw new Error('Candidate left main during upload');
  if (channel === 'server' && !await canPromote()) return 'superseded-draft';
  await request(`${root}/releases/${existing.id}`, 'PATCH', {
    draft: false, prerelease: channel === 'macos', make_latest: channel === 'server' ? 'true' : 'false',
  });
  return 'published';
}

module.exports = { prepare, publish, serverFiles, macFiles };
if (require.main === module) {
  publish({ directory: 'dist', commit: process.env.INFUSER_BUILD_SHA || process.env.GITHUB_SHA,
    channel: process.argv[2], repository: process.env.GH_REPO,
    runUrl: `${process.env.GITHUB_SERVER_URL}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}`,
  }).then(result => console.log(result)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
