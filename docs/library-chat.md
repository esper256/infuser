# Library chat

One shared room per private group for enabled invited members. Open **Chat** beside the online
player badges in the header. The panel opens directly below them, with upward
triangles pointing to visible connected players who appear in the last twelve
messages and have chatted within five minutes. Pointers follow badge replacement,
wrapping and theme changes; offline or offscreen portraits get no pointer.
Toggle Chat again, use × or Escape, or click outside to hide it. Outside clicks
still reach the Library controls underneath; hiding preserves your draft.
Speech bubbles point toward each sender's current profile picture; your messages
sit on the right. The header button shows the exact session-local unread count
in a prominent badge.
While closed with unread messages, it briefly wiggles every eight seconds; reduced
motion keeps the badge without animation. The popup targets 437 × 672 CSS pixels,
clamped to the available viewport, including the phone keyboard.
Initial history is not marked unread. Names and portraits follow
profile changes. The same component works in every Library theme.

Enter sends; Shift+Enter adds a line. Sending failures preserve the draft and
reuse its request ID on retry. Message text is plain text, with no HTML, links,
attachments, direct messages, or game SDK access. Reading older messages does not
force-scroll when new ones arrive. **New messages ↓** returns to the bottom.
Small upward gestures must leave the bottom immediately. Unread badge updates,
unchanged polls and presence repositioning must not pull the reader back down.
The sender and adult admins can remove a message with its × control.

The room holds at most 100 records (including removal tombstones), displays the
last seven days, and prunes older records on the next send. It survives restarts
in a separate SQLite table; legacy catalog rewrites do not erase it. Removed
text is cleared immediately. Disabled members cannot read, send, or delete;
their messages are omitted from the room. Re-enabling them restores any retained
messages. This is recent conversation, not a permanent archive.

## Library endpoints

All require the existing enabled-member session on the Library host. Writes also
require its exact HTTPS Origin; game launch and publisher tokens do not grant access.
Responses are `Cache-Control: no-store` and expose opaque player IDs, never emails.

- `GET /api/chat` → `{messages:[{id, text, created, author:{id,name,avatar}, can_delete}]}`,
  in chronological ID order. `created` is Unix seconds.
- `POST /api/chat` with `{client_id, text}` → `{id}`. Trimmed text is 1–280 Unicode
  characters; newlines are allowed, other control characters are rejected. The
  browser's native maxlength may be stricter for emoji. `client_id` is 16–64 ASCII
  letters/digits/dashes (the UI uses UUIDs). Within retained history, retries of
  the same sender/request return the existing ID, including removed messages.
  Reusing a live request ID with different text returns 409. Each sender may send
  at most ten messages per minute; removal does not reset the limit (429).
- `DELETE /api/chat/ID` → `{ok:true}` for the sender or an adult; otherwise 403.
  Missing messages return 404. Unknown JSON fields and invalid text return 422;
  request bodies over 4 KiB return 413.

The UI polls every three seconds while open and ten seconds while closed,
pauses in hidden tabs, and reconnects without repeated toasts. Requests time out
after twelve seconds. Chat itself stays on Infuser; optional background notifications use the browser’s push service.

## Automatic client updates

The Library HTML embeds a revision derived from the release build SHA and its
shared client assets. `GET /api/client-state` returns `{version, game_open}` for
an enabled member. Every thirty seconds, a visible Library compares revisions.
It reloads only when the server confirms no recent game launch for that member
(including guests, other tabs/devices and native launches), no dialog/menu or
launch/send is active, no chat draft exists, no editor has focus, and there has
been no interaction for five seconds. A game is recent while its launch is
unexpired and has activity within ninety seconds. After a game closes, its last
heartbeat may therefore defer a refresh for up to ninety seconds plus polling.

Offline/errors and malformed responses never trigger a reload. The initial
revision comes from the loaded HTML, not a potentially newer first poll. A page
opened before update detection shipped needs one manual refresh to acquire it.

Checks: `cargo test --locked --test chat_journey` exercises real HTTP/SQLite,
permissions, retry/removal, bounded storage, profile changes, and game activity.
Browser review covers both themes, narrow phones, two member sessions, drafts,
unread state, scrolling, reconnects, and automatic refresh deferrals.
`node tests/chat_scroll_journey.mjs` is the focused browser regression for tiny
upward wheel gestures, keyboard scrolling and history anchoring. It uses Playwright
with a local mock API; set `INFUSER_PLAYWRIGHT` to its module path and optionally
`INFUSER_CHROMIUM` to a system Chromium executable when not using their defaults.

## Home Screen and notifications

Infuser can be installed from the browser menu or **Install Infuser** in the
profile menu. On iPhone/iPad, use **Share → Add to Home Screen**, then open that
icon. The installed website is named **Infuser**; **Infuser Play** remains the
native game launcher.

In your profile menu, turn **Chat notifications** on for each device you want.
Permission is requested only after that tap. On iOS/iPadOS, this requires the
Home Screen app (16.4 or later). If permission was denied, change it in the
browser/device settings before trying again. Turn the same switch off to stop
alerts on that device. Signing out unsubscribes that browser too.

Alerts work while the Library is closed. They say “Your group has a new message”
without including message text. They are quiet, share one notification slot,
and open chat when tapped. Arrival never navigates a window or exits fullscreen.
Delivery is best-effort and subject to the browser, OS notification settings and
network availability. No alerts for your own messages or other private groups.
Operator inspection never subscribes to the inspected group.

The daemon sends standard VAPID-authenticated Web Push to the browser's push
service. It generates `web-push.key` in the private state directory automatically;
back it up with the database. No Firebase project, Apple developer account or
extra process is needed. The key survives updates. Replacing it requires devices
to turn notifications off and on again.

Subscriptions and a coalescing outbox live in separate SQLite tables. Message
retries don't re-notify. Delivery rechecks group membership, enabled accounts and
message removal; dead endpoints are removed. Temporary failures retry up to three
times, and queued work expires after five minutes. Provider-side delivery expires
after one minute, avoiding a burst of stale chat alerts when a phone reconnects.
Browser subscriptions unused for 180 days are removed.

`GET /api/notifications` returns `{public_key}`. `PUT` with `{endpoint}` enables
that device; `DELETE` with `{endpoint}` disables it. These use the same membership,
Origin and read-only inspection rules as chat. Endpoints must belong to supported
Google, Mozilla, Apple or Microsoft push services, use HTTPS and port 443; redirects
are disabled. Endpoints are private credentials and must not be logged. Since
alerts have no payload, no chat content or encryption keys are sent or stored for
push delivery.

The service worker does not cache authenticated pages, API responses, games or
saves. Offline navigation shows a small reconnect page. Existing app revision
checks still govern Library reloads.

Checks: `cargo test --locked --test notifications_journey`. To include the actual
Chromium service worker, notification, fullscreen and sign-out journey, set
`INFUSER_PWA_BROWSER_NODE=/path/to/node` (use the Playwright setup in
[development](development.md)). The test substitutes browser-provider registration;
actual iPhone/Android background delivery must also be checked on a phone.
