# Public platform API

The engine recipes are the preferred entry point. These are **pre-1.0** low-level
building blocks for custom agents; see the [SDK compatibility review](sdk-contract.md).
All bodies/responses are JSON unless noted.
Public traffic goes through HTTPS/Caddy; `/v1/*` is still private to the host.

## Publisher credentials

An adult admin can create an app-scoped connection file. An invited player can
create a new game through **Add a game**, then copy a prompt containing an opaque,
single-use setup code valid for one hour. The agent runs `infuser connect --server
HTTPS_ORIGIN --app APP`, supplying the code on stdin. The CLI exchanges it once
and stores `.local/APP.credentials.json` with private permissions. Each file
contains `api_url`, `app_id`, and `token`. Use `Authorization: Bearer TOKEN`.
Credentials can publish only their game, cannot read player saves, invite members,
or control the host. Admin revocation and member disabling take effect immediately.

| Method | Route | Action |
|---|---|---|
| GET | `/api/publisher/capabilities` | Supported runtimes, architecture, limits |
| POST | `/api/publisher/claim` | Exchange a setup code for permanent app-scoped credentials |
| GET | `/api/publisher/tags` | Current tag catalog |
| PUT | `/api/publisher/apps/APP/tags` | Set this game's tags |
| PUT | `/api/publisher/apps/APP/builds/BUILD` | Create immutable build draft |
| GET | same | Inspect metadata and uploaded file inventory |
| PUT | `/api/publisher/apps/APP/builds/BUILD/files/PATH` | Upload raw file body |
| POST | `/api/publisher/apps/APP/builds/BUILD/publish` | Validate and promote; optional `{"listed":false}` for adult-only preview |
| DELETE | `/api/publisher/apps/APP/builds/BUILD` | Remove an unused build |
| DELETE | `/api/publisher/apps/APP` | Unpublish and expire sessions; retain builds/saves |

Create body: `{"manifest":MANIFEST,"server":null,"spa":false,"cross_origin_isolation":false}`.
Claim is the exception to bearer authentication: send `{"token":"HEX_SECRET","app_id":"APP"}`,
stripping the `infuser_setup_` prefix from the copyable code. Success returns the
connection file; replay fails with 401 and an expired code with 410. A replacement
code invalidates previous unclaimed codes for that app. Never print either secret.

Tags use `{"tags":["puzzle","3d"]}`. Choose up to eight lowercase IDs, 1–32 letters,
numbers or dashes, without leading/trailing dashes. Prefer the catalog; custom
tags are allowed. Manifest `tags` initializes the game's tags at first publication;
later builds preserve author edits. Use the tags endpoint to change them explicitly.

The manifest follows `examples/game-manifest.json`. IDs are lowercase slug strings,
up to 48 characters, beginning with a letter. Upload a UTF-8 `index.html` including
`<script type="module" src="/_infuser/sdk.js"></script>`; the CLI adds it for you.
Files are limited to 64 MiB, builds to 256 MiB/4096 files, and each app to 12 builds.
The installation also has an aggregate content budget and a free-disk reserve.
Drafts can be resumed by checking the file inventory. Published files are immutable.
No arbitrary remote URLs are fetched and no build scripts execute on the host.

An optional advanced `server` object uses the validated game settings documented
in `deploy/infuser.example.toml`. Images must already be staged on the host and
pinned to a digest; image upload/build/pull automation is not yet provided.
For `web:true`, the player endpoint is its game origin plus `/_infuser/server`;
Caddy authenticates and strips this prefix before proxying HTTP/WebSockets. Native
TCP/UDP endpoints use expiring IP admission plus game-validated launch tickets.
Changing a server build returns 202 with `state:"queued"` while its shared server
is active. The immutable upload activates automatically after confirmed shutdown;
the current client/server pair remains available until then. Build inspection
returns `publication_state` (`uploading`, `queued`, `published`, or `retained`).
A newer publication request replaces the pending one. Pending builds cannot be
changed or deleted; unpublishing the app also cancels its pending update. The queue
survives controller restarts. Never force a second server or give players mismatched
clients. An adult force-restart UI is not implemented yet.

## Player website

Google sessions are validated against the loopback OAuth2 Proxy, then checked
against Infuser's own invitation list. Unsafe browser requests require the exact
HTTPS Origin. The proxy must overwrite `X-Infuser-Client-IP` with the connection IP.

- `GET /api/me`, `GET /api/library` — identity, games, friends, reactions.
- `POST /api/my-games` with `{"title":"My Game"}` — returns `app_id`, `api_url`,
  `setup_code`, `expires_at` (Unix seconds), and `tag_catalog`; no permanent credential.
- `POST /api/my-games/APP/setup` — owner/admin replaces an unclaimed setup code.
- `PUT /api/apps/APP/tags` — owner/admin sets `{"tags":["chill","puzzle"]}`.
- `POST /api/apps/APP/launch` with `{"guest":false}` — scoped launch URL; game owns
  mode selection. Legacy explicit `mode` is still accepted. Pre-1.0 warms a declared
  multiplayer backend before launch; in-game on-demand startup is planned.
- `GET /api/launches/ID` — owner-only startup polling/renewal.
- `PUT /api/apps/APP/reaction` with `{"emoji":"🚀","text":"Loved it!"}`; DELETE to remove your own.
- Adult-only `/api/admin/members` GET/POST and `/api/admin/credentials` GET/POST;
  DELETE `/api/admin/credentials/ID` revokes a credential.

## Game SDK and saves

The launch URL contains a short-lived, single-use token in its fragment. A minimal
bootstrap exchanges it for a secure, HttpOnly, host-only game cookie and removes
the fragment before loading any game code. Portal Google cookies never go to
untrusted game hosts. Normal player/app/build hosts are stable; every guest launch
gets a fresh host. A new build gets a different origin so it cannot replace the
cookie or assets in an already-open old build. Cloud saves persist across builds;
browser-local storage is not a cross-version save mechanism.

The browser SDK is `/_infuser/sdk.js`.
It exports `ready`, `saves.load(slot)`, `saves.save(slot,data,version)`, and `leave()`;
it also exposes `window.Infuser`. See the Three.js recipe for usage.
Launches open `/_infuser/play`, with fullscreen and return-to-Library controls in a
slim title bar above a same-origin game frame. The game still loads at `/` with its
own viewport, so fixed canvases and root-relative assets work without rewriting.
The SDK maintains the game session; it adds no controls over the game content.

- `GET /_infuser/context` — player identity, guest flag, app/build, scoped game
  ticket and ready endpoint. Never treat a display name as an identity.
- `POST /_infuser/heartbeat` — renew every 25 seconds, including startup.
- `DELETE /_infuser/heartbeat` — explicit leave. Closing a tab is handled by expiry.
- `GET /_infuser/saves/SLOT` — `{version:0,data:null}` for an empty slot.
- `PUT /_infuser/saves/SLOT` — `{version:EXPECTED,data:JSON}`; returns new version.
  A stale version is 409. Limit 1 MiB/slot, 16 slots/player/game. Add your game's
  schema version inside data. Guest saves expire with the session; personal saves
  survive releases and daemon restarts. Publisher credentials cannot access them.

For native sockets, servers receive `INFUSER_SERVER_TOKEN` and `INFUSER_API_URL`
as runtime environment variables. Verify the client's scoped ticket by POSTing
`{"ticket":"..."}` to `/api/server/verify` with that **server** bearer credential.
The response is game-bound identity and expiry. Authenticate joins, periodically
revalidate, and never accept a publisher token or client-claimed identity.

## Current boundaries

No peer relay/signaling, native client distribution, automatic engine server
packaging, build queues, or multi-branch previews yet. An admin-only preview is an
app visibility setting; it does not create a second server. Guest browser and
cloud saves are isolated; server games must honor the returned guest identity.
