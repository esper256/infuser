// Release notes for one build-<sha> publication. The PR title is the sentence
// families may read; path rules decide whether that sentence is user-visible.
const { execFileSync } = require('node:child_process');

const FAMILY_PREFIX = /^(?:family|user):\s*/i;
const INTERNAL_PREFIX = /^(?:internal):\s*/i;
const CONVENTIONAL = /^(?:feat|fix|chore|docs|test|ci|refactor|perf|style|build)(?:\([^)]+\))?:\s*/i;

// A change is internal only when every file stays in this set. Product code is
// family-visible without a label, which matches pull requests opened today.
function internalPath(file) {
  if (!file || file.endsWith('/')) return false;
  if (/(^|\/)(README|AGENTS|LICENSE)(\.[a-z]+)?$/i.test(file)) return true;
  if (file.endsWith('.md')) return true;
  const prefixes = [
    '.github/', 'docs/', 'tests/', 'deploy/', 'examples/', 'sdk/', 'native-protocol/',
    'web/vendor/', 'desktop/packaging/', 'src/docker/', 'src/server_package/',
  ];
  if (prefixes.some(prefix => file.startsWith(prefix))) return true;
  const files = new Set([
    'Cargo.toml', 'Cargo.lock', 'build.rs', 'desktop/Cargo.toml', 'desktop/Cargo.lock',
    'desktop/build.rs', 'src/updater.rs', 'src/rollback.rs', 'src/backup.rs',
    'src/host_health.rs', 'src/maintenance.rs', 'src/operations.rs', 'src/deployment.rs',
    'src/firewall.rs', 'src/caddy.rs', 'src/publish_capture.rs', 'src/publisher.rs',
    'src/server_package.rs', '.gitignore',
  ]);
  return files.has(file);
}

function audience(title, files, truncated) {
  const raw = String(title || '').trim();
  if (FAMILY_PREFIX.test(raw)) return 'family';
  if (INTERNAL_PREFIX.test(raw)) return 'internal';
  if (truncated) return 'family';
  if (!files || !files.length) return 'internal';
  return files.some(file => !internalPath(file)) ? 'family' : 'internal';
}

function friendly(title) {
  let text = String(title || '').trim();
  text = text.replace(FAMILY_PREFIX, '').replace(INTERNAL_PREFIX, '').replace(CONVENTIONAL, '');
  text = text.replace(/\s*\(#\d+\)\s*$/, '').replace(/\s+/g, ' ').trim();
  if (!text || /^[0-9a-f]{7,40}$/i.test(text)) return null;
  if (/^merge (pull request|branch)\b/i.test(text)) return null;
  text = text.charAt(0).toUpperCase() + text.slice(1);
  if ([...text].length > 160) text = [...text].slice(0, 159).join('').trimEnd() + '…';
  return text;
}

function buildChangelog({ since = null, pulls = [], commits = [] } = {}) {
  const changes = [];
  const seen = new Set();
  for (const pull of pulls) {
    if (!pull || seen.has(pull.number)) continue;
    seen.add(pull.number);
    const text = friendly(pull.title);
    if (!text) continue;
    changes.push({
      text,
      audience: audience(pull.title, pull.files, pull.truncated),
      pr: pull.number,
    });
  }
  for (const commit of commits) {
    if (commit.pr && seen.has(commit.pr)) continue;
    const subject = String(commit.message || '').split('\n')[0];
    if (/^Merge pull request #\d+/.test(subject) || /^Merge branch /.test(subject)) continue;
    const text = friendly(subject);
    if (!text) continue;
    changes.push({
      text,
      audience: audience(subject, commit.files, commit.truncated),
    });
  }
  return { version: 1, since, changes: changes.slice(0, 40) };
}

function notes(changelog) {
  const lines = (audience) => (changelog.changes || [])
    .filter(change => change.audience === audience)
    .map(change => `- ${change.text}`);
  const parts = [];
  const family = lines('family');
  const internal = lines('internal');
  if (family.length) parts.push("## What's new\n" + family.join('\n'));
  if (internal.length) parts.push('## Behind the scenes\n' + internal.join('\n'));
  return parts.join('\n\n');
}

function cli(args) {
  return execFileSync('gh', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}

async function filesFor(request, route) {
  const files = [];
  for (let page = 1; page <= 3; page++) {
    const batch = await request(`${route}${route.includes('?') ? '&' : '?'}per_page=100&page=${page}`) || [];
    for (const file of batch) files.push(file.filename);
    if (batch.length < 100) return { files, truncated: false };
  }
  return { files, truncated: true };
}

async function loadHistory({ repository, sha, request }) {
  if (!/^[0-9a-f]{40}$/.test(sha)) throw new Error('Invalid release commit');
  const latest = await request(`repos/${repository}/releases/latest`);
  let since = null;
  if (latest) {
    const match = /^build-([0-9a-f]{40})$/.exec(latest.tag_name || '');
    if (!match) throw new Error('Unrecognized latest release');
    since = match[1];
  }
  if (!since || since === sha) return buildChangelog({ since, pulls: [], commits: [] });
  const compare = await request(`repos/${repository}/compare/${since}...${sha}`);
  if (!compare || !Array.isArray(compare.commits)) throw new Error('Cannot read commits since the previous release');
  const pulls = [];
  const commits = [];
  const seen = new Set();
  for (const commit of compare.commits.slice(0, 40)) {
    const associated = await request(`repos/${repository}/commits/${commit.sha}/pulls`) || [];
    if (associated.length) {
      for (const pull of associated) {
        if (seen.has(pull.number)) continue;
        seen.add(pull.number);
        const files = await filesFor(request, `repos/${repository}/pulls/${pull.number}/files`);
        pulls.push({ number: pull.number, title: pull.title, files: files.files, truncated: files.truncated });
      }
      continue;
    }
    const detail = await request(`repos/${repository}/commits/${commit.sha}`);
    const listed = detail && Array.isArray(detail.files) ? detail.files.map(file => file.filename) : [];
    commits.push({
      sha: commit.sha,
      message: commit.commit && commit.commit.message,
      files: listed,
      truncated: !!(detail && detail.files && detail.files.length >= 300),
    });
  }
  return buildChangelog({ since, pulls, commits });
}

function writeChangelog(path, changelog) {
  require('node:fs').writeFileSync(path, JSON.stringify(changelog, null, 2) + '\n');
}

async function main() {
  const sha = process.env.INFUSER_BUILD_SHA || process.env.GITHUB_SHA;
  const repository = process.env.GH_REPO || process.env.GITHUB_REPOSITORY;
  if (!repository || !sha) throw new Error('GH_REPO and INFUSER_BUILD_SHA are required');
  const request = (route) => {
    try {
      return JSON.parse(cli(['api', route]));
    } catch (error) {
      if (/\(HTTP 404\)/.test(String(error.stderr || error.message))) return null;
      throw error;
    }
  };
  const changelog = await loadHistory({ repository, sha, request });
  writeChangelog(process.argv[2] || 'CHANGELOG.json', changelog);
  console.log(`changelog: ${changelog.changes.length} change${changelog.changes.length === 1 ? '' : 's'}`);
}

module.exports = {
  audience, friendly, buildChangelog, notes, loadHistory, internalPath,
};

if (require.main === module) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
