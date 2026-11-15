# Readable game URLs

A game's durable link is `https://YOUR_INFUSER/games/moon-racers`. It opens that
game's card, preserving Google sign-in and the adult invitation list. Play and
guest play remain explicit clicks; opening a shared link does not start a server.
The card's arrow menu includes **Copy game link**. Publication responses and the
CLI also return the link. An authenticated resolver is available at
`GET /api/games/SLUG`; it returns `app_id`, canonical `slug`, `url`, and `title`.
Names and aliases resolve within the signed-in player’s private group. Two
groups may use the same slug; sharing a link never grants another group access.
Aliases resolve to the canonical link. Hidden previews remain adult-only;
unpublished games return 404. Anonymous resolution requires sign-in.

## Names and renames

Infuser assigns the slug from the initial title, using lowercase ASCII letters,
numbers and single dashes, up to 24 characters. Names start with a letter.
Unusable/reserved names fall back to `game`; collisions get `-2`, `-3`, etc.
Allocation happens in the catalog transaction, including simultaneous creation.
Existing published games receive a persisted assignment when the database opens.
Future title changes and publishes do not silently change the slug or app ID.

To change a slug deliberately, agents use their existing game-scoped credential:

```http
PUT /api/publisher/apps/APP/slug
Authorization: Bearer GAME_PUBLISHER_TOKEN
Content-Type: application/json

{"slug":"moon-racers"}
```

The owner or an adult can use `PUT /api/apps/APP/slug` with the ordinary website
session and Origin check. No new settings form is required. Invalid/reserved
names return 400; a name belonging to another game returns 409. Core route names
and the `g-` execution prefix are reserved. Each game may retain up to 64 previous
names; reusing its own alias is allowed. Names stay reserved after unpublishing
so another game cannot take over an old link. This is not an app-ID rename and
does not move saves, publishing credentials, reactions, or multiplayer identity.

## Execution isolation

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

Slug and alias records live outside legacy catalog JSON so older catalog writers
cannot erase them. A pre-feature daemon will not resolve the new links; restoring
a supported daemon restores the same assignments without reallocating them.
