# Library chat

One shared room for enabled invited members. Open **Chat** beside the online
player badges in the header. The panel opens directly below them, with upward
triangles pointing to visible connected players who appear in the last twelve
messages and have chatted within five minutes. Pointers follow badge replacement,
wrapping and theme changes; offline or offscreen portraits get no pointer.
Toggle Chat again, use × or Escape, or click outside to hide it. Outside clicks
still reach the Library controls underneath; hiding preserves your draft.
Speech bubbles point toward each sender's current profile picture; your messages
sit on the right. The header button shows a session-local unread badge.
Initial history is not marked unread. Names and portraits follow
profile changes. The same component works in every Library theme.

Enter sends; Shift+Enter adds a line. Sending failures preserve the draft and
reuse its request ID on retry. Message text is plain text, with no HTML, links,
attachments, direct messages, or game SDK access. Reading older messages does not
force-scroll when new ones arrive. **New messages ↓** returns to the bottom.
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
after twelve seconds. No external messaging provider or new dependency is used.

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
