// Cheap Linux gate: batch Mac work against published, tested server commits.
const { execFileSync, spawnSync } = require('node:child_process');
const fs = require('node:fs');
const SHA = /^[0-9a-f]{40}$/;
const requiredAssets = ['infuser-macos-amd64', 'infuser-macos-arm64', 'release.json', 'SHA256SUMS'];

// The publisher links the shared root Rust crate. Keep this deliberately broad
// rather than guessing which server modules the linker discards. SDK and fixture
// changes can affect the real publisher journey. UI/docs/desktop-only edits skip.
function relevant(file) {
  return /^(src|native-protocol|sdk|examples|\.cargo)\//.test(file) ||
    /^(Cargo\.(toml|lock)|build\.rs|rust-toolchain(\.toml)?)$/.test(file) ||
    /^tests\/(publisher_journey\.rs$|support\/|fixtures\/)/.test(file) ||
    file === '.github/workflows/macos-publisher.yml' ||
    /^\.github\/scripts\/(mac-release-plan|publish-release)(\.test)?\.cjs$/.test(file);
}
function publishedMac(release) {
  const match = /^macos-([0-9a-f]{40})$/.exec(release.tag_name || '');
  if (!match || release.draft || !release.published_at || release.target_commitish !== match[1]) return null;
  if (!requiredAssets.every(name => release.assets?.some(a => a.name === name && a.state === 'uploaded' && a.size > 0 && /^sha256:[0-9a-f]{64}$/.test(a.digest || '')))) return null;
  return match[1];
}
function plan({ event, latest, releases }, git) {
  if (!['schedule', 'workflow_dispatch'].includes(event)) throw new Error('Unsupported trigger');
  const match = /^build-([0-9a-f]{40})$/.exec(latest?.tag_name || '');
  if (!match || latest.draft || latest.prerelease || !latest.published_at) throw new Error('No published server release');
  const commit = match[1];
  if (!git.ancestor(commit)) throw new Error('Published server commit is not in main history');
  const successful = new Set(releases.map(publishedMac).filter(Boolean));
  if (successful.has(commit)) return { build: false, commit, reason: 'Exact Mac release already published' };
  if (event === 'workflow_dispatch') return { build: true, commit, reason: 'Manual build of latest published server commit' };
  // API release ordering is not commit ordering: an old rerun may publish later.
  const baseline = git.history(commit).find(sha => successful.has(sha));
  if (!baseline) return { build: true, commit, reason: 'No published Mac ancestor' };
  const changed = git.changed(baseline, commit).filter(relevant);
  return { build: changed.length > 0, commit, baseline, reason: changed.length ? 'Mac inputs changed' : 'Mac inputs unchanged', changed };
}
function gitAdapter() {
  const read = args => execFileSync('git', args, { encoding: 'utf8' });
  return {
    ancestor(commit) {
      if (!SHA.test(commit)) throw new Error('Invalid commit');
      const result = spawnSync('git', ['merge-base', '--is-ancestor', commit, 'refs/remotes/origin/main']);
      if (result.error || ![0, 1].includes(result.status)) throw new Error('Cannot establish main ancestry');
      return result.status === 0;
    },
    history: commit => read(['rev-list', '--topo-order', commit]).trim().split('\n'),
    // --no-renames exposes both old and new paths, so moving a relevant input
    // outside the allowlist still counts. NUL delimiters preserve unusual names.
    changed: (base, head) => read(['diff', '--no-renames', '--name-only', '-z', base, head, '--']).split('\0').filter(Boolean),
  };
}
module.exports = { plan, relevant, publishedMac, gitAdapter };
if (require.main === module) {
  try {
    if (process.env.GITHUB_REPOSITORY !== 'esper256/infuser' || process.env.GITHUB_REF !== 'refs/heads/main') throw new Error('Run only on trusted main');
    const api = args => JSON.parse(execFileSync('gh', ['api', ...args], { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }));
    const latest = api(['repos/esper256/infuser/releases/latest']);
    const pages = api(['--paginate', '--slurp', 'repos/esper256/infuser/releases?per_page=100']);
    const result = plan({ event: process.env.GITHUB_EVENT_NAME, latest, releases: pages.flat() }, gitAdapter());
    console.log(JSON.stringify(result, null, 2));
    fs.appendFileSync(process.env.GITHUB_OUTPUT, `build=${result.build}\ncommit=${result.commit}\n`);
    fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `Mac publisher: ${result.reason}. Candidate: \`${result.commit}\`.\n`);
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
