# Readable game URLs

Audience: game authors and their agents.

A game's durable link is `https://YOUR_INFUSER/games/moon-racers`. It opens that
game's card, preserving Google sign-in and the invitation list. Play and
guest play remain explicit clicks; opening a shared link does not start a server.
The card's arrow menu includes **Copy game link**. Publication responses and the
CLI also return the link. An authenticated resolver is available at
`GET /api/games/SLUG`; it returns `app_id`, canonical `slug`, `url`, and `title`.
Names and aliases resolve within the signed-in player’s private group. Two
groups may use the same slug; sharing a link never grants another group access.
Aliases resolve to the canonical link. Hidden previews remain admin-only;
unpublished games return 404. Anonymous resolution requires sign-in.

## Names and renames

Infuser assigns the slug from the initial title, using lowercase ASCII letters,
numbers and single dashes, up to 24 characters. Names start with a letter.
Unusable/reserved names fall back to `game`; collisions get `-2`, `-3`, etc.
Future title changes and publishes do not silently change the slug or app ID.

To change a slug deliberately, agents use their existing game-scoped credential:

```http
PUT /api/publisher/apps/APP/slug
Authorization: Bearer GAME_PUBLISHER_TOKEN
Content-Type: application/json

{"slug":"moon-racers"}
```

The owner or an admin can use `PUT /api/apps/APP/slug` with the ordinary website
session and Origin check. Invalid/reserved
names return 400; a name belonging to another game returns 409. Core route names
and the `g-` execution prefix are reserved. Each game may retain up to 64 previous
names; reusing its own alias is allowed. Names stay reserved after unpublishing
so another game cannot take over an old link. This is not an app-ID rename and
does not move saves, publishing credentials, reactions, or multiplayer identity.

Admin tools rename the Library title only. They do not change the slug. A new
play origin is `slug--g-…` on the wildcard host, so a slug change would strand
browser storage on the old origin, and Infuser does not redirect that origin.
The publisher and owner slug endpoints above remain available for a deliberate
address change; old `/games/SLUG` aliases keep resolving, but new launches use
the new prefix. Deleting a hidden game removes its slug and aliases entirely.
Old links then 404, and a future game may reuse the address. There is no redirect.

## Play URLs and share links

Actual play origins look like `moon-racers--g-OPAQUE_ID.YOUR_INFUSER`. The readable
prefix identifies the game, while the opaque component still isolates players,
builds and individual guest/Join friend launches. Do not strip the component or
host game code on the main website origin. The label fits a normal wildcard
certificate and wildcard DNS record; no per-game DNS changes are needed.

Existing random `g-OPAQUE_ID` origins and already-open named origins continue to
work for their authenticated launch, including after a rename. New launches use
the canonical slug. Guests still start with separate storage; cloud saves remain
keyed by player and immutable app ID. Share the durable `/games/SLUG` link, not an
expiring launch URL containing credentials or a player's isolated origin.
