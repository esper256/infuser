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

- `GET /api/chat` → `{messages:[{id, text, created, author:{id,name,avatar}, can_delete}]}`,
  in chronological ID order. `created` is Unix seconds.
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

## Notifications

`GET /api/notifications` returns `{public_key}`. `PUT` with `{endpoint}` enables
that device; `DELETE` with `{endpoint}` disables it. These use the same membership,
Origin and read-only inspection rules as chat. Endpoints must belong to supported
Google, Mozilla, Apple or Microsoft push services, use HTTPS and port 443; redirects
are disabled. Endpoints are private credentials and must not be logged. Since
alerts have no payload, no chat content or encryption keys are sent or stored for
push delivery.
