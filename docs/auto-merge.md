# Automatic merging of owner PRs

Audience: Infuser developers.

`Merge owner PRs` implements automatic merging without requiring approvals or
branch protection. Native GitHub auto-merge alone does not do this: enabling the
repository feature only allows individual PRs to opt in, and its waiting behavior
relies on required reviews/checks. This workflow uses the merge API instead and
does not change repository security settings or enable native auto-merge.

## Activation and stopping

1. Merge this proposal into `main` when ready. It is inert by default.
2. Optionally run **Merge owner PRs** on **main**, leaving **dry_run** checked.
   This reads live PRs and logs candidates without merging or starting a release.
3. Set the repository Actions variable `OWNER_AUTO_MERGE` to the literal `true`
   when automatic merging and the resulting release builds are wanted.

Activation includes existing eligible PRs, not just newly opened ones. The draft
proposal does not activate the workflow. No secret/PAT, approval requirement,
branch protection, or repository auto-merge setting is needed. Repository/organization
Actions policy must permit the declared job permissions and pinned actions.

Remove the variable or set it to `false` to stop future merge jobs. Cancel an
already running merge job as well if an immediate stop is necessary. A dispatch
with `dry_run: false` cannot bypass the variable. A dispatch from a non-main ref
is skipped. Dry-run still uses the job's write-capable token, but its code makes
no write calls.

## Behavior

- Polls every five minutes (offset from the top of the hour); GitHub scheduling
  can be delayed or dropped. Manual dispatch with dry_run unchecked can run sooner.
  GitHub may disable scheduled workflows in public repos after 60 days of inactivity.
- Examines open PRs to `main`, oldest first, and merges at most one per run.
- Trusts only author `esper256`, numeric GitHub user ID `528155`, with both base
  and head in `esper256/infuser`. Forks and other authors, including dependency
  bots, are excluded. This trusts the owner's PR content, not every individual
  commit author; collaborators who can push to those branches remain trusted.
- Skips drafts, closed/merged PRs, conflicts, blocked/unknown/behind merge states,
  and PRs updated less than one minute ago. Unknown states are retried next run.
- Checks all available latest check runs and latest legacy statuses, with
  pagination, on both the PR head and GitHub's synthetic merge commit.
  Checks must be completed with success, neutral, or skipped; statuses must be
  success. Pending, failed, cancelled, timed-out, or action-required checks wait.
  A rerun supersedes the previous result as exposed by GitHub's latest-check API.
- No checks is allowed: this does not introduce required CI. A workflow job that
  intentionally reports success despite failed steps (for example advisory CI)
  cannot be distinguished from success here. Post-main release tests cannot
  prevent a merge. This policy tests reported results, not the project's code.
- Re-fetches PR state and current main before writing, then passes the exact
  inspected head SHA to GitHub's merge API. Uses a merge commit, preserves branch
  history, and does not delete branches, resolve conflicts, update branches,
  request reviews, or override protections that may be added later.
- Serializes its own runs. API errors fail closed; no release is dispatched if
  the merge fails. A blocked candidate does not prevent examining later PRs.
- Executes only the script from main with persisted checkout credentials disabled.
  It never checks out PR code, runs PR commands, or downloads candidate artifacts.

## Release behavior and limits

A `GITHUB_TOKEN` merge does not trigger a normal push workflow. After one successful
merge, the script explicitly dispatches existing `release.yml` on `main`, which
builds and publishes according to that workflow's existing rules. This proposal
leaves `release.yml` unchanged. It does not introduce a server deployment step.
Other push-only workflows will **not** automatically run for the bot merge.

Dispatch uses the current main ref, not a pinned merge SHA, matching the existing
latest-main release strategy. If dispatch fails after a merge, the merge is
already complete and the job fails visibly: manually rerun **Build and release**
on main. Re-running the merger is not a release-retry mechanism and could merge
the next eligible PR. A runner cancellation between merge and dispatch has the
same recovery procedure.

Without enforced required checks, GitHub cannot atomically combine this script's
checks, draft/base inspection, and merge. A check may appear or change after the
inspection, or main/PR metadata may change just before the request. The head SHA
is server-guarded; the base/check snapshot is not. The one-minute settling window
reduces registration races but does not eliminate them. Strong guarantees require
branch protection/rules or a merge queue, deliberately outside this proposal.
The owner can still merge manually regardless of this advisory automation policy.

## Validation

Run `node --test .github/scripts/auto-merge.test.cjs`. The read-only
`Auto-merge policy tests` PR workflow runs these tests on relevant changes,
including drafts. Validate workflow syntax with actionlint. Unit tests mock writes;
live read-only inspection can validate API eligibility, but actual merging and
release dispatch are intentionally not exercised while preparing the draft.

References: [GitHub native auto-merge](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/automatically-merging-a-pull-request),
[GITHUB_TOKEN workflow triggering](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow),
[scheduled event limitations](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule).
