# Update a game's presentation without a build

An agent with the game's existing publisher connection can update its Library
title and cover independently of the game files. No game source, export, rebuild,
SDK change, or session restart is needed. This is the path for adding Firefly
Drift's cover to an already-published game.

Check `GET /api/publisher/capabilities` for `game_metadata.version: 1` first.
Use the existing connection's `api_url`, `app_id`, and bearer `token`. Never
create a replacement game/app ID just to change its artwork. Keep the connection
file in ignored `.local/`, and never print the token or commit it.

## Endpoints

| Method | Path | Body and effect |
| --- | --- | --- |
| GET | `/api/publisher/apps/APP/metadata` | Inspect effective title, cover URL, overrides, tags, slug, active build, and listed status |
| PATCH | `/api/publisher/apps/APP/metadata` | `{"title":"Firefly Drift"}` sets the Library title; `{"title":null}` restores the build's title |
| PUT | `/api/publisher/apps/APP/card-art` | Raw `image/webp` bytes replace the independent cover override |
| DELETE | `/api/publisher/apps/APP/card-art` | Clear the override and use the active build's cover, or the Library's abstract fallback |
| PUT | `/api/publisher/apps/APP/tags` | Existing `{"tags":["chill","arcade"]}` endpoint; no build required |
| PUT | `/api/publisher/apps/APP/slug` | Existing readable-URL rename endpoint; old aliases remain valid |

Title edits trim surrounding whitespace and accept 1–80 Unicode characters,
without control characters. Omitted/unknown patch fields are rejected. These
endpoints do not change the game's own title screen or runtime manifest, player
handles, author identity, listing, gameplay modes, SDK declarations, or builds.

Covers follow the [card art guide](card-art-guide.md): exactly 1024×1024, static,
opaque RGB WebP, sRGB with the embedded profile removed, at most 250 KiB. Invalid
images return 422; oversized request bodies return 413. Invalid replacements
leave the current cover intact. Existing installation storage limits apply.

Every metadata mutation returns the same view as GET, including:

```json
{
  "app_id": "firefly-drift",
  "build": "existing-build",
  "listed": true,
  "title": "Firefly Drift",
  "title_override": null,
  "card_art_url": "/api/apps/firefly-drift/card-art/CONTENT_SHA256",
  "card_art_override": "CONTENT_SHA256",
  "updated": 1234567890,
  "tags": ["chill"],
  "slug": "firefly-drift"
}
```

The example IDs are illustrative; use the ID in the real connection file.
`updated` is the presentation metadata timestamp, not a game-build update.
`card_art_override` is null when using build art or the generic fallback. Image
URLs remain member-authenticated; hidden games' covers remain adult-only. A
publisher token authorizes upload and inspection, not a player browser session.

## Precedence, persistence, and rollback

An explicit title/cover override wins over the active build and survives later
game publications, queued server updates, restarts, and unpublishing. Unpublished
games and their art remain inaccessible to players. Clear the override to follow
build metadata again. Uploads replace one bounded stored image per game; they do
not accumulate immutable build copies or historical covers. Keep your source art
in the game project so you can upload a prior image again.

Artwork bytes and their content revision commit together in SQLite. A changed
image produces a changed Library URL, and a superseded override URL returns 404.
The normal Library refresh picks up the new image; refresh the page to see it
immediately. No session is expired, no server is restarted, and saves are untouched.
The metadata table is separate from the legacy build catalog so older CLI catalog
writes do not erase overrides.

## Agent upload example

Run from the game project with a private connection file and a validated cover.
This uses only Python's standard library, keeps the token out of command-line
arguments/output, checks capability support, and refuses redirects:

```sh
python3 - .local/GAME.credentials.json infuser-art/card-hero.webp <<'PY'
import hashlib, json, pathlib, sys, urllib.parse, urllib.request

connection = json.loads(pathlib.Path(sys.argv[1]).read_text())
origin = connection['api_url'].rstrip('/')
assert urllib.parse.urlsplit(origin).scheme == 'https', 'HTTPS required'
app = urllib.parse.quote(connection['app_id'], safe='')

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None

opener = urllib.request.build_opener(NoRedirect())
def request(path, method='GET', data=None):
    headers = {'Authorization': 'Bearer ' + connection['token']}
    if data is not None:
        headers['Content-Type'] = 'image/webp'
    req = urllib.request.Request(origin + path, data=data, headers=headers, method=method)
    with opener.open(req, timeout=30) as response:
        return json.load(response)

caps = request('/api/publisher/capabilities')
assert caps.get('game_metadata', {}).get('version') == 1, 'Update the Infuser server first'
cover = pathlib.Path(sys.argv[2]).read_bytes()
assert len(cover) <= 250 * 1024, 'Cover exceeds 250 KiB'
result = request('/api/publisher/apps/' + app + '/card-art', 'PUT', cover)
assert result['card_art_override'] == hashlib.sha256(cover).hexdigest()
print('Cover updated. Active build:', result['build'])
PY
```

Verify the image in the signed-in Library after uploading. Do not claim a
production update merely because an art file was committed to a repository.
