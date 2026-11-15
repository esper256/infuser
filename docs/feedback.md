# Public feedback

The Library's Profile menu contains **Send feedback**. Feature request is the
default; it includes no diagnostics. Bug reports optionally include six strictly
typed browser observations: online status, secure-context status, presence of
WebGL, WebRTC and WebAssembly APIs, and viewport width bucket (small <600px,
medium <1000px, otherwise large). These are capability flags, not guarantees that
the features work. No raw errors, console history, URLs, origins, user agents,
account details, game data, storage, tokens, or setup codes are collected.

Users write a title (80 characters) and description (2000 characters), review the
complete public title/body and diagnostic snapshot, then explicitly consent and
choose **Post public issue**. Children are asked to review with an adult. Free text
is not automatically redacted; the warning and preview are essential. Nothing is
sent to GitHub before that final action. Drafts live only in the page's memory.

## Operator setup

Create a fine-grained GitHub token restricted to `esper256/infuser` with Issues:
[write permission](https://docs.github.com/en/rest/issues/issues#create-an-issue) only, using an account authorized to create issues there. Put the token
in `feedback.github-token` in the controller's state directory, readable only by
the service user (mode 0600). Never commit it or serve that directory as static
files. Token rotation/removal takes effect on the next request, without restart.
No deployment workflow change is required. Without this file submission is disabled;
users can still prepare and review a draft. No child needs a GitHub account.

Reports are posted by the token's account to the fixed Infuser repository. The
server never forwards the member identity, cookies or origin to GitHub. The
normal authenticated, enabled-member and same-origin write checks apply;
inspection sessions cannot submit. Requests are capped at 8 KiB, and unknown
fields/diagnostic values are rejected. The server permits one attempt per member
per hour and ten total attempts per installation per rolling day. Failures consume
an attempt. A small durable `feedback-attempts.json` ledger stores timestamps and
hashed member identifiers, never feedback content. Keep it when restarting or
restoring the service. Run only one controller per state directory (the normal
Infuser model); the reservation lock is in-process.

Submission has no automatic retry. A network failure or non-201 GitHub response
may have an uncertain outcome; the preview stays available and asks an adult to
check GitHub before resubmitting. The page does not claim success without a 201.
Closing/reloading loses drafts. There is no offline queue, attachment upload,
private support inbox, moderation queue, or automatic error capture. Limits are
per installation; operators should also monitor the shared repository for abuse.

Run browser coverage with `node tests/feedback_journey.mjs` (Playwright installed,
or `INFUSER_PLAYWRIGHT` set to its module path; optional `INFUSER_CHROMIUM`).
