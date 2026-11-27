# Chat and notification API

Audience: website API clients.

These member-only APIs are separate from publishing and the game SDK.

## Chat

All require the existing enabled-member session on the Library host. Writes also
require its exact HTTPS Origin; game launch and publisher tokens do not grant access.
Responses expose opaque player IDs, never emails. Chat reads use
`Cache-Control: private, no-cache` and an ETag. Send `If-None-Match` to revalidate;
an unchanged authorized view returns 304. Membership and group access are checked
before every response, including 304. Mutations use `no-store`.

- `GET /api/chat` → `{messages:[{id, text, created, author:{id,name,avatar}, can_delete}], reads:[{id, name, avatar, message}]}`,
  messages in chronological ID order. `created` is Unix seconds. Each `reads`
  entry is one person's cursor: the id of the newest message that was on screen
  in their open chat while the page was visible. The cursor only moves forward.
  `id` is the opaque player id.
- `POST /api/chat/read` with `{message}` → `{message}`. The stored cursor is the
  greater of the requested id and the cursor already stored. A message the
  caller cannot see, including one outside their visit floor, returns 404.
  Inspection is read-only. The response `message` is the cursor after the call.
- `POST /api/chat` with `{client_id, text}` → `{id}`. Trimmed text is 1–280 Unicode
  characters; newlines are allowed, other control characters are rejected. The
  browser's native maxlength may be stricter for emoji. `client_id` is 16–64 ASCII
  letters/digits/dashes (the UI uses UUIDs). Within retained history, retries of
  the same sender/request return the existing ID, including removed messages.
  Reusing a live request ID with different text returns 409. Each sender may send
  at most ten messages per minute; removal does not reset the limit (429).
- `DELETE /api/chat/ID` → `{ok:true}` for the sender or an admin; otherwise 403.
  Missing messages return 404. Unknown JSON fields and invalid text return 422;
  request bodies over 4 KiB return 413.

## House calls

`GET /api/house-calls` returns only `{images:[{id,label}]}` for shipped images
that are enabled and allowed for the current profile in its own group. An off,
not-allowed, visiting or inspecting view returns an empty list. No topic,
tags, message templates, cooldowns, profile allowlists or quiet hours are in
this response.

`POST /api/chat` also accepts `{client_id,house_call:IMAGE_ID}`. It shares the
normal channel authorization, rate limit, retry ID, retention and deletion path.
A house call cannot include text or a screenshot. Unknown, disabled or disallowed
IDs return 422. Stored cards appear in chat as `house_call:{id,label}` with
`house_call_triggered:true` only if the server queued a notification in the same
transaction. Cooldown, quiet-hours and full-queue suppression still save the card
with a false flag. Clients must celebrate only new live messages, never history.

`GET /api/admin/house-calls` requires an admin of that group and returns
`{topic_set,images,profiles,quiet_hours}`. Each image has
`{id,enabled,label,tag,message,cooldown_minutes,profiles:[PROFILE_ID]}`; `profiles`
at the top level contains eligible local `{id,name}` choices. `quiet_hours` is
null or `{start:"HH:MM",end:"HH:MM",timezone:IANA_ZONE}`. The topic URL is never
returned, including to admins and inspection sessions from other groups.

`PUT /api/admin/house-calls` accepts `{images,quiet_hours,topic_url?}`. Supply every
shipped image; omitted `topic_url` preserves the secret, a nonempty HTTP(S) URL
replaces it, and `""` clears it. The same member/Origin/inspection rules apply to
writes; no operator privilege substitutes for membership and admin rights in
that group. Cooldowns are 0–10080 minutes, labels 1–80 characters, tags up to 80
ASCII letters/digits/dashes/underscores, and message templates up to 4096 UTF-8
bytes. `{{profile}}` expands literally to the authenticated sending profile's
display name; the expanded body is capped at a UTF-8 boundary at 4096 bytes.

The per-group settings, cooldown reservations and bounded outbox live in the
server SQLite state. A background worker POSTs the tag in the ntfy `Tags` header
and expanded text as the body, with redirects disabled and a three-second timeout.
There is one attempt; failures log no URL, request or response body. Quiet hours
are checked both when queueing and before dispatch. Claimed requests are not
replayed after a crash. Only shared image assets are public under
`/assets/house-calls/ID.png`.

## Notifications

`GET /api/notifications` returns `{public_key}`. `PUT` with `{endpoint}` enables
that device; `DELETE` with `{endpoint}` disables it. These use the same membership,
Origin and read-only inspection rules as chat. Endpoints must belong to supported
Google, Mozilla, Apple or Microsoft push services, use HTTPS and port 443; redirects
are disabled. Endpoints are private credentials and must not be logged. Since
alerts have no payload, no chat content or encryption keys are sent or stored for
push delivery.

`POST /api/notifications/test` with `{endpoint}` sends a normal empty push to
a subscription already registered to the current profile. Success means the
provider accepted the request. Each profile can send one test every 30 seconds.
