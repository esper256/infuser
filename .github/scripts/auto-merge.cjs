// Run only from trusted main. Never check out or execute a candidate PR.
const REPOSITORY = 'esper256/infuser';
const OWNER_ID = 528155;

function eligible(pr, now = Date.now()) {
  return pr.state === 'open' && !pr.draft && !pr.merged &&
    pr.user?.id === OWNER_ID && pr.user?.login === 'esper256' &&
    pr.head?.repo?.full_name === REPOSITORY &&
    pr.base?.repo?.full_name === REPOSITORY && pr.base.ref === 'main' &&
    pr.mergeable === true && ['clean', 'unstable', 'has_hooks'].includes(pr.mergeable_state) &&
    now - Date.parse(pr.updated_at) >= 60_000;
}

function checksPass(checks, statuses) {
  // Checks API filter=latest handles re-runs. For legacy statuses keep the newest
  // entry per context, including when an older failure lives on a later page.
  const latest = new Map();
  for (const status of statuses) {
    if (!latest.has(status.context) || status.id > latest.get(status.context).id) {
      latest.set(status.context, status);
    }
  }
  return checks.every(c => c.status === 'completed' &&
    ['success', 'neutral', 'skipped'].includes(c.conclusion)) &&
    [...latest.values()].every(s => s.state === 'success');
}

async function run({ github, context, core, dryRun = true, now = Date.now }) {
  const { owner, repo } = context.repo;
  if (`${owner}/${repo}` !== REPOSITORY) throw new Error('Unexpected repository');
  const common = { owner, repo };
  const prs = await github.paginate(github.rest.pulls.list,
    { ...common, state: 'open', base: 'main', sort: 'created', direction: 'asc', per_page: 100 });
  for (const candidate of prs) {
    const params = { ...common, pull_number: candidate.number };
    const { data: pr } = await github.rest.pulls.get(params);
    if (!eligible(pr, now())) {
      core.info(`#${pr.number}: ineligible, unsettled, or mergeability unknown`);
      continue;
    }
    // pull_request jobs normally attach to the synthetic merge SHA, while other
    // integrations attach to head. Inspect both, not just the branch's checks.
    if (!pr.merge_commit_sha) continue;
    let passing = true;
    for (const ref of new Set([pr.head.sha, pr.merge_commit_sha])) {
      const checks = await github.paginate(github.rest.checks.listForRef,
        { ...common, ref, filter: 'latest', per_page: 100 });
      const statuses = await github.paginate(github.rest.repos.listCommitStatusesForRef,
        { ...common, ref, per_page: 100 });
      if (!checksPass(checks, statuses)) passing = false;
    }
    if (!passing) {
      core.info(`#${pr.number}: checks pending or unsuccessful`);
      continue;
    }
    const { data: fresh } = await github.rest.pulls.get(params);
    const { data: base } = await github.rest.repos.getBranch({ ...common, branch: 'main' });
    if (!eligible(fresh, now()) || fresh.head.sha !== pr.head.sha ||
        fresh.base.sha !== pr.base.sha || fresh.merge_commit_sha !== pr.merge_commit_sha ||
        base.commit.sha !== pr.base.sha) {
      core.info(`#${pr.number}: changed during inspection; retry next run`);
      continue;
    }
    if (dryRun) {
      core.info(`#${pr.number}: WOULD merge ${pr.head.sha} (no writes)`);
      continue;
    }
    // The server enforces the exact inspected head and conflicts/protection.
    const { data: result } = await github.rest.pulls.merge({
      ...params, sha: pr.head.sha, merge_method: 'merge',
    });
    if (!result.merged) throw new Error(`#${pr.number}: merge rejected: ${result.message}`);
    core.info(`#${pr.number}: merged ${result.sha}`);
    // GITHUB_TOKEN merges do not trigger push workflows. Explicit dispatch is
    // supported and preserves release.yml's latest-main behavior unchanged.
    await github.rest.actions.createWorkflowDispatch({
      ...common, workflow_id: 'release.yml', ref: 'main',
    });
    core.info('Dispatched release.yml on main');
    // Only one merge per run: give GitHub time to recalculate remaining PRs.
    return;
  }
}

module.exports = { eligible, checksPass, run };
