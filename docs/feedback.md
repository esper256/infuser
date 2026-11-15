# Private feedback and adult review

**Profile → Send feedback** sends a private draft to the member's own group.
Feature request is the default, without diagnostics. Members preview and consent
to sharing the draft with adults; this action never contacts GitHub. Private
intake works even when GitHub posting has not been configured.

**Profile → Review feedback** is available to enabled adult admins. Adults see
only their home group's queue, can edit the title, description and type, remove
diagnostics, or dismiss a report. The adult previews the exact public title/body,
checks an explicit public-posting consent box, and chooses **Post public issue**.
Server-side adult, enabled-member, same-origin and group checks enforce this;
children cannot publish by calling the API directly. Operator inspection cannot
read or act on any review queue. No submitter identity is sent to GitHub.

Bug diagnostics are optional and strictly typed: online status, secure-context
status, presence of WebGL/WebRTC/WebAssembly APIs, and viewport width bucket
(small <600px, medium <1000px, otherwise large). Adults review the original
snapshot, never a replacement from their own device. These flags do not guarantee
that the capabilities work. No raw errors, console logs, URLs, origins, user
agents, account details, game data, storage, tokens, or setup codes are collected.
Free text is not automatically redacted: the adult must remove private details.
Only the adult-edited version is posted. There is no attachment upload.

## Operator setup

Use a fine-grained token restricted to `esper256/infuser` with
[Issues write permission](https://docs.github.com/en/rest/issues/issues#create-an-issue).
Store it in `<state_dir>/feedback.github-token`, mode 0600, owned by the Infuser
service user. The default path is `/var/lib/infuser/feedback.github-token`.
Never commit or serve the state directory. Rotation/removal takes effect on the
next request. Without the token, adults can review/dismiss but cannot publish.
The issue is posted by the token's account to the fixed Infuser repository.
No child or reviewing adult needs their own GitHub account.

## Storage, limits and retention

The private `feedback-queue.json` file uses Infuser's existing atomic, mode-0600
file replacement. It stores the group, hashed submitter ID, random request/entry
identifiers, report and state. The list API does not expose submitter identifiers.
Pending and uncertain reports expire 30 days after intake. Successful posting or
dismissal immediately deletes the report text and diagnostic snapshot. Small
hashed deduplication tombstones remain until the same 30-day deadline. Cleanup
runs during normal controller maintenance (about every minute) and on queue
access. Stopping the controller pauses cleanup; backups require their own
retention policy. Corrupt storage fails closed for feedback and never triggers a
public submission. Retention errors are logged without report content.

Intake allows 3 new reports per member per rolling day, 50 per installation per
day, and 500 retained entries including tombstones. Requests are capped at 16 KiB;
titles at 80 characters and descriptions at 2000. Unknown fields are rejected.
Public posting allows one attempt per group per minute and ten per installation
per rolling day. Failed upstream attempts consume that budget. Limits persist
across restarts. Run one controller per state directory, as with normal Infuser.

## Retries and uncertain outcomes

The client retries private intake with the same request ID and frozen draft;
replays return the original entry. Identical drafts from the same member/group
are also deduplicated for the retention window, including dismissed reports.
Reusing a request ID with different content returns 409. Unsent client drafts
are only in page memory; closing/reloading the form discards them.

Before contacting GitHub, the server durably records the approved version and
marks the entry as attempted. Only one caller can claim a pending entry. A
successful retry of a confirmed submission returns success without another POST,
provided the approved content matches. GitHub errors, lost responses or a crash
leave an uncertain/in-progress entry that cannot be posted again, even after a
restart. Adults should check GitHub, then dismiss it; if absent, deliberately
create a revised report. There is no automatic public retry or reconciliation.
An in-flight entry cannot be dismissed for its first minute. This conservative
at-most-once policy favors avoiding duplicate public issues over automatic recovery.

Tests: `cargo test --locked --lib portal::feedback`,
`cargo test --locked --test feedback_journey`, and
`node tests/feedback_journey.mjs` with Playwright installed (or
`INFUSER_PLAYWRIGHT` set to its module path; optional `INFUSER_CHROMIUM`).
