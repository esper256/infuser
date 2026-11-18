# Game descriptions, art requests, and covers

Audience: game authors and their agents.

Use the game's existing publisher connection. These edits do not rebuild the
game, restart sessions, or change saves. Check `GET /api/publisher/capabilities`
for `game_metadata.version: 1`; descriptions additionally require
`game_metadata.description_max_chars`. Art requests require
`game_metadata.art_prompt_max_chars`.

The metadata response also includes the active build’s `orientation`. To change
it, set `orientation` to `landscape` or `portrait` in `infuser.game.json` and
publish the build. Existing games default to landscape. On phones already held the right way, Infuser requests the lock on launch.
Otherwise, the first turn into the game’s orientation requests fullscreen and
locks rotation where supported. If the browser requires a tap, use **Fullscreen**.
After you exit fullscreen, turning the phone again leaves it windowed.

## Endpoints

Authenticate with the connection's bearer token, scoped to its `app_id`.

| Method | Path | Effect |
| --- | --- | --- |
| GET | `/api/publisher/apps/APP/metadata` | Read title, description, art prompt, cover, tags, slug, build, and visibility |
| PATCH | `/api/publisher/apps/APP/metadata` | Set any of `title`, `description`, `art_prompt` |
| PUT | `/api/publisher/apps/APP/card-art` | Replace the independent cover with raw `image/webp` bytes |
| DELETE | `/api/publisher/apps/APP/card-art` | Remove the override; use build art or the fallback |
| PUT | `/api/publisher/apps/APP/tags` | Set `{"tags":["chill","arcade"]}` |
| PUT | `/api/publisher/apps/APP/slug` | Rename the readable URL; old aliases remain valid |

The game must have a published build, which may be hidden. Read metadata before
editing it. `title` accepts 1–80 characters, `description` 1–2000; both trim outer
whitespace and reject control characters. Use a single plain-text paragraph.
An omitted field is unchanged. `title:null` restores the build title;
`description:null` clears the description. Empty patches and unknown fields fail.
An invalid field rejects the entire patch. `art_prompt` accepts 1–4000 characters,
trims outer whitespace, and permits newlines but no other control characters.
`art_prompt:null` cancels the request. Omitted prompts remain unchanged.

Write a short factual description after first publication: what players do,
setting, and distinctive visual features. Derive it from the project or creator's
brief, not the title alone. Keep it useful to another agent preparing artwork;
do not invent features. Later agents should read and preserve it unless the game
has changed. The description is returned to signed-in members in the Library API,
and to authorized publishers here; it is not public anonymous metadata.

Example patch:

```json
{"description":"Race tiny hovercraft through moonlit canals, alone or with friends. Low-poly boats and glowing buoys define the scene."}
```

## Either an image or a prompt

For every game type and engine, the agent should **either upload high-quality artwork meeting the spec or
store an art prompt**, not improvise an inferior placeholder. Prefer the prompt
when high-quality image generation is not your strength. Do not spend time staging
screenshots; only an exceptionally readable, well-composed capture with no UI,
HUD, or browser chrome qualifies as cover art. The game can publish immediately;
an Infuser art agent can fulfill the prompt later.

Keep `description` factual. Use `art_prompt` for the image brief: recognizable
subjects, setting, composition, style, mood, and exclusions. Make it understandable
without the source repository. The Infuser art agent also follows the shared
[art spec](card-art-guide.md), so do not paste the whole spec into each prompt.

```json
{"art_prompt":"Moon Racers: low-poly hovercraft skimming through moonlit canals, glowing buoys reflected on dark water. Fast and playful, with a bold readable boat silhouette. No text or interface elements."}
```

Prompts are private metadata for authorized publishers and the all-games metadata
service key. They are not returned by the player Library API or displayed in the
UI. `art_prompt_pending:true` marks a request awaiting artwork. Completed prompts
remain stored with `art_prompt_pending:false`; null means no brief. Setting a
prompt, even to the same text, requests artwork again. Do not promise
a turnaround time. For the Infuser-side workflow, see [art-agent access](art-agent.md).

Every mutation returns the same shape as GET:

```json
{
  "app_id": "moon-racers",
  "build": "build-123",
  "listed": true,
  "title": "Moon Racers",
  "title_override": null,
  "description": "Race tiny hovercraft through moonlit canals.",
  "art_prompt": null,
  "art_prompt_revision": null,
  "art_prompt_pending": false,
  "card_art_url": "/api/apps/moon-racers/card-art/CONTENT_SHA256",
  "card_art_override": "CONTENT_SHA256",
  "updated": 1234567890,
  "tags": ["racing"],
  "slug": "moon-racers"
}
```

## Artwork

Follow the [card art guide](card-art-guide.md): static opaque 1024×1024 RGB WebP,
sRGB with the profile stripped, at most 250 KiB. Invalid images return 422;
oversized requests return 413. The old image remains if validation fails.

Using the private curl auth file prepared by the [HTTP publishing recipe](publishing-http.md#connect-once),
set `API` to the connection's HTTPS origin and `APP` to its app ID:

```sh
curl --silent --show-error --fail --proto '=https' \
  --config .local/publisher.curl --request PUT \
  --header 'Content-Type: image/webp' \
  --data-binary @infuser-art/card-hero.webp \
  "$API/api/publisher/apps/$APP/card-art"
```

For a description or art prompt, write the patch JSON to `.local/metadata.json`, then:

```sh
curl --silent --show-error --fail --proto '=https' \
  --config .local/publisher.curl --request PATCH \
  --header 'Content-Type: application/json' \
  --data-binary @.local/metadata.json \
  "$API/api/publisher/apps/$APP/metadata"
```

Verify the returned metadata with the publisher credential. Inspect the signed-in
Library only if an authorized player session is already available; its absence
does not block completion. Do not ask for a Google login to check artwork.
Committing an image to a repository does not upload it to Infuser. Cover URLs
require player membership; a publisher token is not a browser login.

## Persistence

Descriptions, art prompts and their completion state, and title/cover overrides survive publications, restarts, and
unpublishing. An explicit cover overrides build art until DELETE clears it.
Ordinary cover uploads do not complete a pending request. The [art-agent completion
request](art-agent.md#fulfill-a-request) uploads and marks its matching prompt fulfilled
atomically, retaining the brief for future reference.
Only one override image is retained; keep source art in the game project if you
want to restore an earlier image. Replacements produce a new cover URL; obsolete
override URLs return 404. Library refresh picks up the change.

These edits do not change the game's title screen, manifest, ownership, gameplay
modes, build selection, or visibility.

The member Library exposes only `card_art_pending`, true when the game has no
active cover and an unfulfilled prompt. The UI uses a shared “Generating...”
placeholder; private prompt text and revisions remain restricted to metadata APIs.
