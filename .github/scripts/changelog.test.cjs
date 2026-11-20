const { test } = require('node:test');
const assert = require('node:assert/strict');
const { audience, friendly, buildChangelog, notes, loadHistory } = require('./changelog.cjs');

test('friendly titles drop hashes, merge subjects, and conventional prefixes', () => {
  assert.equal(friendly('feat(web): Games remember where you left off (#12)'), 'Games remember where you left off');
  assert.equal(friendly('family: You can react to a friend\'s game'), 'You can react to a friend\'s game');
  assert.equal(friendly('a'.repeat(40)), null);
  assert.equal(friendly('Merge pull request #12 from esper256/games'), null);
  assert.equal(friendly(''), null);
});

test('path classification treats product code as family-visible and CI, docs, and tests as internal', () => {
  assert.equal(audience('Remember cloud saves', ['src/cloud.rs']), 'family');
  assert.equal(audience('Refresh the release workflow', ['.github/workflows/release.yml', 'docs/ci.md']), 'internal');
  assert.equal(audience('Fix a flaky journey', ['tests/update_journey.rs']), 'internal');
  assert.equal(audience('internal: Reformat the library page', ['web/clubhouse.js']), 'internal');
  assert.equal(audience('family: Explain a play change', ['docs/play-updates.md']), 'family');
  assert.equal(audience('Touch both', ['src/updater.rs', 'web/clubhouse.js']), 'family');
  assert.equal(audience('Huge change', ['docs/ci.md'], true), 'family');
});

test('one line per pull request, commits inside it are not repeated, and internal lines stay marked', () => {
  const log = buildChangelog({
    since: 'a'.repeat(40),
    pulls: [
      { number: 12, title: 'feat: Games remember where you left off', files: ['src/cloud.rs', 'web/clubhouse.js'] },
      { number: 13, title: 'ci: Cache the release build', files: ['.github/workflows/release.yml'] },
    ],
    commits: [
      { message: 'Remember cloud saves\n\nMore detail', files: ['src/cloud.rs'], pr: 12 },
      { message: 'Merge pull request #12 from esper256/saves', files: ['src/cloud.rs'], pr: 12 },
      { message: 'Fix the health check copy', files: ['src/portal/groups.rs'] },
    ],
  });
  assert.deepEqual(log.changes.map(change => [change.audience, change.text]), [
    ['family', 'Games remember where you left off'],
    ['internal', 'Cache the release build'],
    ['family', 'Fix the health check copy'],
  ]);
  assert.match(notes(log), /## What's new\n- Games remember where you left off\n- Fix the health check copy/);
  assert.match(notes(log), /## Behind the scenes\n- Cache the release build/);
  assert.doesNotMatch(notes(log), /[0-9a-f]{40}/);
});

test('an internal-only release has no family section', () => {
  const log = buildChangelog({
    pulls: [{ number: 4, title: 'Document rollback', files: ['docs/update-rollback.md'] }],
  });
  assert.equal(notes(log), '## Behind the scenes\n- Document rollback');
  assert.equal(log.changes[0].audience, 'internal');
});

test('history since the previous build tag is loaded once per pull request', async () => {
  const previous = 'a'.repeat(40), sha = 'b'.repeat(40);
  const routes = [];
  const request = async (route) => {
    routes.push(route);
    if (route.endsWith('/releases/latest')) return { tag_name: `build-${previous}` };
    if (route.includes('/compare/')) return { commits: [
      { sha: '1'.repeat(40), commit: { message: 'Merge pull request #9 from esper256/play' } },
      { sha: '2'.repeat(40), commit: { message: 'docs: only docs' } },
    ] };
    if (route.endsWith('/commits/' + '1'.repeat(40) + '/pulls')) return [{ number: 9, title: 'You can see when a friend is playing' }];
    if (route.includes('/pulls/9/files')) return [{ filename: 'web/clubhouse.js' }];
    if (route.endsWith('/commits/' + '2'.repeat(40) + '/pulls')) return [];
    if (route.endsWith('/commits/' + '2'.repeat(40))) return { files: [{ filename: 'docs/ci.md' }] };
    throw new Error('unexpected ' + route);
  };
  const log = await loadHistory({ repository: 'esper256/infuser', sha, request });
  assert.equal(log.since, previous);
  assert.deepEqual(log.changes.map(change => change.text), [
    'You can see when a friend is playing',
    'Only docs',
  ]);
  assert.equal(log.changes[0].audience, 'family');
  assert.equal(log.changes[1].audience, 'internal');
  assert.equal(routes.filter(route => route.includes('/pulls/9/files')).length, 1);
});

test('the first release and an already-published commit do not invent history', async () => {
  const sha = 'b'.repeat(40);
  const empty = await loadHistory({
    repository: 'esper256/infuser', sha,
    request: async (route) => route.endsWith('/releases/latest') ? null : {},
  });
  assert.deepEqual(empty.changes, []);
  const same = await loadHistory({
    repository: 'esper256/infuser', sha,
    request: async () => ({ tag_name: `build-${sha}` }),
  });
  assert.deepEqual(same.changes, []);
});
