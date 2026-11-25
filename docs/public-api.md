# Public platform API

Audience: game authors and API tool authors.

For a new game, start with the [creation guide](create-game.md); for an existing
game, follow [integration and publishing](agents.md). This page supplies
**pre-1.0** low-level building blocks for custom tools. Return to the checklist
after looking up the request you need here.
All bodies/responses are JSON unless noted.
New integrations should use [versioned SDK/API routes](sdk-versioning.md).
Game routes below use major 1 under `/_infuser/api/v1`; older unversioned routes
remain available for existing builds.
Public traffic goes through HTTPS/Caddy; `/v1/*` is still private to the host.
Member-only [chat and notification APIs](chat-api.md) are
separate from the game SDK and publisher credentials.

## Publisher credentials

For a complete upload using existing tools, see [Publishing over HTTP](publishing-http.md).
This is a supported alternative to the CLI, including on Mac and Windows.

An admin can create an app-scoped connection file. An invited player can
create a new game through **Add a game**, then copy a prompt containing an opaque,
single-use setup code valid for three hours. The agent runs `infuser connect --server
HTTPS_ORIGIN --app APP`, supplying the code on stdin. The CLI exchanges it once
and stores `.local/APP.credentials.json` with private permissions. Each file
contains `api_url`, `app_id`, and `token`. Use `Authorization: Bearer TOKEN`.
Credentials can publish only their game, cannot read player saves, invite members,
or control the host. Admin revocation and member disabling take effect immediately.

Publishing requests can return `503` with `Retry-After` when upload capacity is
busy. Wait that many seconds and retry the same request. The publisher CLI does
this automatically with a bounded retry count. A lost connection is different:
inspect build status before deciding whether to repeat an operation.

| Method | Route | Action |
|---|---|---|
| GET | `/api/publisher/capabilities` | Supported runtimes, architecture, limits |
| POST | `/api/publisher/claim` | Exchange a setup code for permanent app-scoped credentials |
| GET | `/api/publisher/tags` | Current tag catalog |
| PUT | `/api/publisher/apps/APP/slug` | Rename the durable URL; preserve old aliases |
| PUT | `/api/publisher/apps/APP/tags` | Set this game's tags |
| GET / PATCH | `/api/publisher/apps/APP/metadata` | Read/edit title, description, and art prompt |
| PUT / DELETE | `/api/publisher/apps/APP/card-art` | Upload/clear a cover override without a build |
| PUT | `/api/publisher/apps/APP/builds/BUILD` | Create immutable build draft |
| GET | same | Inspect metadata and uploaded file inventory |
| PUT | `/api/publisher/apps/APP/builds/BUILD/files/PATH` | Upload raw file body |
| PUT | `/api/publisher/apps/APP/builds/BUILD/server/files/PATH` | Upload private managed-server source |
| POST / GET | `/api/publisher/apps/APP/builds/BUILD/server-build` | Seal/queue server packaging, or inspect its state/logs |
| GET | `/api/publisher/apps/APP/server-diagnostics` | Private live/recent runtime logs, health, exit status and build correlation |
| GET | `/api/publisher/apps/APP/diagnostic-events` | Read this game’s recent [browser and server events](server-diagnostics.md#browser-and-game-timeline) |
| POST | `/api/publisher/apps/APP/builds/BUILD/publish` | Validate and promote; optional `{"listed":false}` for admin-only preview. The first publish of a new game can return 409 `duplicate_title` |
| DELETE | `/api/publisher/apps/APP/builds/BUILD` | Remove an unused build |
| DELETE | `/api/publisher/apps/APP` | Unpublish and expire sessions; retain builds/saves |

The capabilities `architecture` field describes the **server**, not the agent
machine. Choose a CLI download using the machine running the publishing tool.

Create body: `{"manifest":MANIFEST,"server":null,"spa":false,"cross_origin_isolation":false}`.
When `server_packaging.enabled` is advertised, `server_recipe` can replace the
advanced `server` image object. See [managed packaging](server-packaging.md) and
the [Node agent recipe](engines/node-server.md). These two server inputs are mutually
exclusive. Server source never enters the public web export. Finalize packaging
after uploading both client and server files; publish only when its state is `ready`.
Claim is the exception to bearer authentication: send `{"token":"HEX_SECRET","app_id":"APP"}`,
stripping the `infuser_setup_` prefix from the copyable code. Success returns the
connection file; replay fails with 401 and an expired code with 410. A replacement
code invalidates previous unclaimed codes for that app. Never print either secret.

Tags use `{"tags":["puzzle","3d"]}`. Choose up to eight lowercase IDs, 1–32 letters,
numbers or dashes, without leading/trailing dashes. Prefer the catalog; custom
tags are allowed. Manifest `tags` initializes the game's tags at first publication;
later builds preserve author edits. Use the tags endpoint to change them explicitly.

See [readable game URLs](game-urls.md) for authenticated resolution and naming.
Publishing responses include the canonical `slug` and `url`.

The manifest follows `examples/game-manifest.json`. IDs are lowercase slug strings,
up to 48 characters, beginning with a letter. Upload a UTF-8 `index.html` including
`<script type="module" src="/_infuser/sdk/v1.js"></script>` for a major-1
declaration, or `/_infuser/sdk.js` for legacy builds; the CLI adds it for you.
Files are limited to 64 MiB, builds to 256 MiB/4096 files, and each app to 12 builds.
The installation also has an aggregate content budget and a free-disk reserve.
Drafts can be resumed by checking the file inventory. Published files are immutable.
No arbitrary remote URLs are fetched and no build scripts execute on the host.

Optional card art uses the ordinary file upload at `infuser-art/card-hero.webp`.
`capabilities.card_art` advertises `path`, `width`, `height`, `max_bytes`,
`format`, and `derivatives` (the smaller widths a cover URL can serve). See the [card art guide](card-art-guide.md) for the image constraints;
invalid images return 422. Bundled art belongs to the immutable build and activates
with it. For artwork, title, or description changes, use the [metadata API](game-metadata.md),
advertised by `capabilities.game_metadata.version = 1`. The metadata response
includes the active build’s `orientation`. Declare `portrait` or `landscape`
in the build manifest when `game_metadata.orientations` is advertised.
Omitting it selects landscape. A raw WebP PUT to the
app-level `card-art` endpoint applies immediately without touching builds or
sessions. DELETE clears that override. Metadata PATCH accepts `{"title":"Name"}`
or `{"title":null}` to restore the build title. After an admin renames the game,
`title_locked` is true and a different title, or `title:null`, returns 409. Sending
the locked title again changes nothing. Omit `title` to edit the description or
art prompt. The next publish keeps the locked Library name and does not copy the
manifest title over it. Description accepts
(1–2000 plain-text characters) or `description:null` when capabilities include
`game_metadata.description_max_chars`. Read existing metadata before editing.
`art_prompt` (up to 4000 characters, or null to cancel) stores a private pending
art request when `game_metadata.art_prompt_max_chars` is advertised. Prompts are
not included in player responses. Optional model authorship fields —
`author_model` (1–120 characters), `effort_level` (1–40 characters), and
cumulative `estimated_prompts` (integer `1`–`1000000`), each nullable to clear —
are accepted when advertised in `game_metadata` and shown on the Library game
detail popup; agents should omit any field they do not reliably know rather than
guessing. Descriptions, model authorship, prompts, and overrides survive
later publications; none requires a manifest field.

Publishing keys inherit their creator’s [private group](private-groups.md).
Manifests cannot choose or change a group.

An operator can provision a [group or all-games metadata key](art-agent-operations.md). It can list
`GET /api/metadata/apps`, read/PATCH `/api/metadata/apps/APP`, and PUT/DELETE
`/api/metadata/apps/APP/card-art` across games within the key’s scope. It cannot publish builds, access
saves, or administer members. Ordinary game keys cannot enumerate other games.

An optional advanced `server` object uses the validated game settings documented
in `deploy/infuser.example.toml`. Images must already be staged on the host and
pinned to a digest; image upload/build/pull automation is not yet provided.
For `web:true`, the player endpoint is its game origin plus `/_infuser/server`;
The platform authenticates and strips this prefix before proxying HTTP/WebSockets. Native
TCP/UDP endpoints use expiring IP admission plus game-validated launch tickets.
The first publish of an app that has never had a ready build compares the manifest
title with current Library names in the same private group, including hidden games
and an admin's locked name. Comparison trims leading and trailing whitespace and
ignores case. Internal spaces, punctuation, and accents are not folded, so
`Pond` matches ` pond ` and `POND`, while `Pond Life` does not match `Pond  Life`
or `Pond-Life`. A match returns 409:

```json
{"error":"A game with this title already exists: \"Pond\" (id pond, address pond, hidden). ...","code":"duplicate_title","matches":[{"app_id":"pond","title":"Pond","slug":"pond","hidden":true}]}
```

`matches` names each existing game's title, id, slug when it has one, and whether
it is hidden. Publish an update to that game instead. The build draft is kept, so
the same publish can be retried with `{"allow_duplicate_title":true}` when a
separate copy is intentional. The CLI flag is `--allow-duplicate-title`. An update
to an app that already has a published or ready build never returns this warning,
even when the new manifest title matches another game. Capabilities advertise
`game_metadata.duplicate_titles.version`.

Publishing returns 202 with `state:"queued"` while the game has an active managed
server or peer room. The upload activates after that server shuts down or the
peer session ends;
the current client/server pair remains available until then. Build inspection
returns `publication_state` (`uploading`, `queued`, `published`, or `retained`).
A newer publication request replaces the pending one. Pending builds cannot be
changed or deleted; unpublishing the app also cancels its pending update. The queue
survives controller restarts. Never force a second server or give players mismatched
clients. An admin force-restart UI is not implemented yet.

For peer-hosted games, `sdk.capabilities` includes `peers.rooms.v1` when
independent rooms are available. The legacy `session_policy: "one_per_app"`
field describes the managed server process. Room capacity and admission are
per gameplay session. See [peer rooms](peer-hosting.md).

## Player website

These routes require a signed-in, invited Google account. Requests that change
data also require the exact website HTTPS Origin. A publisher credential cannot
be used as a player login.

- `GET /api/me`, `GET /api/library` — identity, games, friends, reactions.
  Library games include `author: {id, name, avatar}` from the existing owner/member records,
  or `null` for an unrecorded owner. `id` is the same opaque player ID used for
  presence; no email is exposed. `avatar` is a curated picture ID or `null` for
  initials. Friends and `/api/me` include the same optional `avatar` field.
  Each game's `players` entries include `{id, player_id, name, avatar, guest}` for allowed,
  enabled sessions seen within the last 90 seconds. Card badges show these active
  sessions, combining duplicate signed-in sessions and labeling guests separately.
  `player_id` matches `join_targets[].player_id`, including each guest’s distinct
  session identity; `id` remains the profile ID used for the portrait.
  `/api/me` also returns `avatars: [{id, name}]`, the available picture catalog,
  and `appearance`, the profile's saved look (`null` until saved, or
  `{"theme":"nightglass","schemes":{"nightglass":"amber-hearth"},"custom":{"nightglass":{"primary":"#122128","accent":"#b5ead6","secondary":"#e8d5a4"}}}`).
  `GET /api/profiles` includes each profile's `appearance` as well so the
  profile chooser can pre-cache the selected profile's theme before navigation.
  Its mixed display order is stable across sessions; new pictures slot in without
  changing existing pictures' relative order. Clients should preserve this order.
  Library games include `description` (nullable plain text) and `card_art_url`
  (null for the fallback). Use the returned cover URL as-is. An override URL's
  last segment is the content hash. A build cover's URL adds that file's content
  hash after the build id, because a deleted build id can be reused. Those
  content-addressed URLs are `private, max-age=31536000, immutable`. A legacy
  build URL without the hash still works and keeps revalidating. `?w=384`,
  `?w=640`, or `?w=768` returns a smaller WebP of the same image; any other `w`
  is 400. The stored master stays 1024 and is not rewritten. Images require
  membership. Hidden images require admin membership; inactive or replaced image
  URLs return 404.
- `GET /api/profile` — returns `{"users":[{"name":"Alex","avatar":"circuit-fox"}]}`
  for profile-picker usage badges. Requires active membership; includes enabled
  accounts in the current group, including offline accounts, with a selected
  picture. Initials-only accounts are omitted. Operator inspection uses the
  inspected group. No emails or account permissions are exposed. The picker
  refreshes this information when opened; shared pictures remain selectable.
- `PUT /api/profile` with `{"avatar":"circuit-fox"}` — saves the signed-in member's
  picture; `{"avatar":""}` restores initials. Returns `{"avatar":ID_OR_NULL}`.
  Unknown IDs return 400; missing/null values and extra fields are rejected.
  Authentication, active membership, and the exact HTTPS Origin are required.
  There is no target-user field, file upload, or remote image URL.
  `GET /assets/avatars/ID/128.webp` and `256.webp` serve embedded public artwork.
  Member updates preserve the selection; accounts without one show initials.
- `GET /api/profile/appearance` — returns `{"appearance": ...}` (`null` if unset)
  for the active profile (selected via `X-Infuser-Profile` when the account holds
  multiple profiles).
- `PUT /api/profile/appearance` with `{"theme":"nightglass","schemes":{},"custom":{}}` —
  saves the active profile's theme, optional Nightglass color scheme, and optional
  custom `#RRGGBB` colors (`primary`, `accent`, `secondary`). Unknown theme IDs,
  unknown scheme IDs, themes that do not support schemes or custom colors, invalid
  hex colors, and unknown JSON fields return 400. Authentication, active
  membership, and the exact HTTPS Origin are required; read-only operator
  inspection returns 403. Saved rows are isolated per profile and removed if the
  profile is deleted.
- `POST /api/my-games` with `{}` — reserves a publishing connection for an existing
  project without asking the user for a name. Returns `app_id`, `api_url`,
  `setup_code`, `expires_at` (Unix seconds), `supported_engines`, and `tag_catalog`;
  no permanent credential. The agent supplies the real title at publication,
  which assigns the readable URL. Optional `title` retains the older workflow.
- `POST /api/my-games/APP/setup` — owner/admin replaces an unclaimed setup code.
- `PUT /api/apps/APP/tags` — owner/admin sets `{"tags":["chill","puzzle"]}`.
- `POST /api/apps/APP/launch` with `{"guest":false}` — scoped launch URL; game owns
  mode selection. Legacy explicit `mode` is still accepted. Managed builds with
  `manifest.server_start:"on_demand"` start at their title screen without a server;
  builds without that declaration retain launch-time warming.
  Opted-in peer and dedicated games also accept an optional [Join friend intent](launch-intents.md).
- `GET /api/launches/ID` — owner-only startup polling/renewal.
- `PUT /api/apps/APP/reaction` with `{"emoji":"🚀","text":"Loved it!"}`; DELETE to remove your own.
- `DELETE /api/admin/apps/APP/reactions/PLAYER` removes a reaction and its note.
  Requires an admin in their home view, with same-origin protection. `PLAYER` is
  the opaque reaction author ID from the Library. Library games expose
  `can_moderate_reactions`; the website offers deletion through each reaction’s
  **⋯** menu in game details. Moderation affects only the current group’s deployment,
  including borrowed games; it never deletes the source group’s reactions.
- Admin-only `PUT /api/admin/apps/APP/visibility` with `{"listed":true}` or
  `{"listed":false}` changes the active game's visibility without publishing a build.
  Requires the exact HTTPS Origin. In the full game popup's **Admin actions** (⋯)
  dropdown, choose **Hide game** or **Unhide game**. Hidden games remain available to admins but are omitted
  from player Libraries and reject new player launches. Existing sessions continue.
  Admin hides persist through pending/future build publications, even when the publisher
  requests `listed:true`. Metadata and build updates remain available through the API.
  **Unhide game** clears the admin hide and shows the latest active build.
  Library games expose `can_manage_visibility` only for admins managing their own group's games.
- Admin-only `PUT /api/admin/apps/APP/title` with `{"title":"Night Pond"}` changes the
  Library name and locks it. The name is trimmed, 1–80 characters, without control
  characters. The slug, app id, saves, and play address stay the same. A later
  agent publish keeps this name. The response includes `title_locked:true` and
  `slug_unchanged:true`. In **Admin tools → Games**, choose **Rename**.
- Admin-only `GET /api/admin/apps/APP/deletion` describes what deletion removes:
  releases and web files, saves, screenshots, leaderboards (none are stored
  separately; scores in saves go with the saves), reactions, cloud checkpoints,
  publishing connections, publish captures, dedicated-server containers, world
  volumes, packaged images, and the address. Operator-supplied images that Infuser
  did not build are left on the host. `blocked` explains when delete will refuse.
- Admin-only `POST /api/admin/apps/APP/deletion` with `{"confirm":"Night Pond"}`
  deletes a hidden game after the admin types its current name, ignoring spaces
  at the ends. A visible game returns 409. A live room, a running dedicated server,
  or a publish still in progress also returns 409; delete does not stop them.
  The record lists who deleted which game and when in `admin-audit.jsonl` in the
  state directory. In **Admin tools → Games**, **Delete** is available only for a
  hidden game and shows the removal list before the name is typed.
- `GET /api/admin/games` lists the signed-in admin's published games with `id`,
  `title`, `slug`, `hidden`, and `title_locked`.
- Admin-only `/api/admin/members` GET/POST and `/api/admin/credentials` GET/POST;
  DELETE `/api/admin/credentials/ID` revokes a credential.
- Admin-only `PUT /api/admin/members/handle` with `{"email":"player@example.com","name":"New handle"}`
  renames an existing member. Requires the exact HTTPS Origin. Handles are trimmed,
  1–40 characters, without control characters. Returns `{player_id, name}`; preserves
  identity, permissions, enabled status, avatar, and saves. Unknown members return 404.
  Ordinary players cannot rename themselves or anyone else; `/api/profile` accepts
  avatar selection only. The admin opens their profile picture menu, then **Admin tools → Members → Rename**.

## Game SDK and saves

The launch URL contains a short-lived, single-use token in its fragment. A minimal
bootstrap exchanges it for a secure, HttpOnly, host-only game cookie and removes
the fragment before loading any game code. Portal Google cookies never go to
untrusted game hosts. Normal player/app/build hosts are stable; every guest launch
gets a fresh host. A new build gets a different origin so it cannot replace the
cookie or assets in an already-open old build. Cloud saves persist across builds;
browser-local storage is not a cross-version save mechanism.

For new integrations the browser SDK is `/_infuser/sdk/v1.js`; legacy builds
retain `/_infuser/sdk.js`.
It exports `ready`, `saves.load(slot)`, `saves.save(slot,data,version)`, and `leave()`;
it also exposes `window.Infuser`. See the Three.js recipe for usage.
Launches open `/_infuser/play`, with fullscreen and return-to-Library controls in a
slim title bar above a same-origin game frame. The game still loads at `/` with its
own viewport, so fixed canvases and root-relative assets work without rewriting.
The SDK maintains the game session; it adds no controls over the game content.

For opt-in managed backends, [runtime sessions](runtime-sessions.md) add
`runtime.acquire()`, `runtime.status()`, and `runtime.release(sessionId)`:
`GET/POST /_infuser/api/v1/runtime/v1` and `DELETE /_infuser/api/v1/runtime/v1/SESSION_ID`.
These endpoints do not select a gameplay mode, create parallel servers, or end
the browser/cloud-save session. Peer-hosted games use the separate peer API.

[Dedicated sessions](dedicated-sessions.md) adds `sessions.connect()` and
`sessions.disconnect()` for verified Node/WebSocket room admission, exact joins,
reconnects and session discovery. Its `sessions.v1` and `connections.websocket.v1`
capabilities are explicit opt-ins; legacy runtime routes retain their behavior.

`presence.get()` reads `GET /_infuser/api/v1/presence/v1` without acquiring anything.
It separates online members, launches with this game open, and active peer-room
participants. Peer hosts use `PUT /_infuser/api/v1/peers/PEER/joinability` with
`{ "epoch": "SESSION_ID", "joinable": true }`. A targeted join sends
`expected_epoch` to `POST /_infuser/api/v1/peers`. To host a fresh room, send
`create: true` and a new `connection_id`. Admission starts open unless the host
supplies `joinable: false`.
See the [presence contract](presence.md) for admission, expiry and compatibility.

`launchIntent.consume()` uses `POST /_infuser/api/v1/launch-intent/v1/consume`. It returns
the launch's intent at most once; ordinary launches and repeated reads return
`{"intent":null}`. The game handles it after loading and retains a title-screen
fallback. Targeted peer joins can supply both `expected_epoch` and `expected_player`.

- `GET /_infuser/api/v1/context` — player identity, guest flag, app/build, scoped game
  ticket and ready endpoint. Never treat a display name as an identity.
- `POST /_infuser/api/v1/heartbeat` renews the launch at startup and every 25 seconds.
  SDK v1 also sends one when the game becomes visible again. The five-minute
  liveness deadline controls whether the launch counts as open. New member web
  launches can save and resume for seven days after exchange or the latest
  heartbeat. Guest, visit, native and older launches keep their existing expiry.
  Signing out ends launches across the account’s profiles and devices.
- If the backend stops, the heartbeat reports HTTP 200 with state `"ended"`.
  After the dead lease is cleared, runtime status reads `"idle"`. Offer to
  reconnect or choose a mode. The launch can still save. The player handles
  `update_available` (a game build update) and `client_update` (Infuser web
  client version and emergency reload generation).
- SDK v1 stops heartbeating and signals `infuser-session-error` once on HTTP
  401, 403 or 410 from a platform request. A 404 or 409 on a heartbeat is also
  terminal. A save conflict (409) remains recoverable. The player provides a
  link to the Library. Temporary failures keep the usual heartbeat schedule.
- `DELETE /_infuser/api/v1/heartbeat` stops announcing play. A member web
  launch retains its save authorization. Closing a tab is handled by expiry.
- `GET /_infuser/api/v1/saves/SLOT` — `{version:0,data:null}` for an empty slot.
- `PUT /_infuser/api/v1/saves/SLOT` — `{version:EXPECTED,data:JSON}`; returns new version.
  A stale version is 409. Limit 1 MiB/slot, 16 slots/player/game. Add your game's
  schema version inside data. Guest saves expire with the session; personal saves
  survive releases and daemon restarts. Publisher credentials cannot access them.

For native sockets, servers receive `INFUSER_SERVER_TOKEN` and `INFUSER_API_URL`
as runtime environment variables. Verify the client's scoped ticket by POSTing
`{"ticket":"..."}` to `/api/server/verify` with that **server** bearer credential.
The response is game-bound identity and expiry. Authenticate joins, periodically
revalidate, and never accept a publisher token or client-claimed identity.

## Current boundaries

Peer-hosted WebRTC signaling and TURN are available when configured; relay
fallback is mandatory. The [PeerJS adapter](peerjs.md) additionally requires the
`peers.peerjs.v1` capability and shared PeerServer service.
The [native Godot preview](engines/godot-native.md) adds verified client delivery
through the desktop companion. It currently accepts client-only single-player
games on Windows/Linux x86-64; native multiplayer is not supported.

Native pairing uses `/api/native/v1/device/start`, explicit signed-in
`/api/native/v1/device/approve`, and verifier-protected `/api/native/v1/device/token`.
Signed-in `POST /api/native/v1/play/APP` accepts `platform` and `guest`, returning
a one-use `infuser://` launch link. The companion claims it at
`POST /api/native/v1/claim` using its account-bound device credential. Devices can
be listed/revoked under `/api/native/v1/devices`; an app can revoke its own key at
`POST /api/native/v1/device/revoke`. Game agents should use `infuser publish` and
the engine adapter rather than implementing this companion protocol themselves.

Managed Node source packaging has a bounded, durable build queue when enabled.
Other engine recipes and multi-branch previews remain out of scope.
An admin-only preview is an app visibility setting; it does not
create a second server. Guest browser/native directories and cloud saves are
isolated; server games must honor the returned guest identity.
