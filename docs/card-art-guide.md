# Infuser card hero art

Audience: game authors and their agents.

This guide applies to **all games**, regardless of engine, web/native delivery,
or multiplayer mode. It specifies the image for a Library card. `infuser publish` automatically includes the
delivery image described below. No artwork field in `infuser.game.json` is needed.

Read the game description from the [metadata API](game-metadata.md), then inspect
available project assets or screenshots. If neither gives enough context, use the
creator's brief; do not guess gameplay from the title. **If high-quality image
generation is not your strength, prefer a detailed `art_prompt` using the
[metadata API](game-metadata.md).** Describe the real game, recognizable subjects,
composition, palette, style, mood, and exclusions for an Infuser art agent.
This is a complete art handoff: publish with a fallback and continue; do not spend
time staging screenshots to avoid requesting art. Suitable existing high-quality
artwork is also welcome.

A gameplay screenshot is an exception: use it only if it reads exceptionally well
as a cover, including at thumbnail size, and contains no UI, HUD, menus, browser
chrome, or window borders. Merely capturing a running game is not enough.

## What to make

Create one compelling, full-bleed image that identifies **this game** at a glance.
It sits behind Infuser's title, creator portrait, tags, and compact Play controls.
The image is artwork, not a screenshot of a finished card.

Start from the actual game: inspect its environments, characters, objects,
mechanics, screenshots, and existing art. Pick one recognizable subject or moment.
Keep its defining shapes and palette. An environment, vehicle, object, or abstract
pattern can lead the image; a character is not required. Do not invent a mascot,
mode, location, or feature to make the game appear more elaborate than it is.

Make a **faithful cover illustration**: lighting, framing, and finish may be more
polished than a gameplay frame, but recognizable characters, objects, environments,
and the game's core identity must match the actual project. Do not substitute a
generic attractive scene for the game's distinctive features. A different rendering
technique is acceptable when those features remain recognizable; an exact screenshot
look is not required. Use a strong silhouette, clear depth, and a few well-chosen
highlights. Pixel art, low-poly, illustrated, graphic, and other deliberate styles
are welcome.
Infuser's dark interface is the frame, not a requirement to turn every game into
a neon city. Bright, warm, or daylight scenes can work too.

Make the artwork compelling and respectful of its players. Avoid patronizing
depictions, corporate stock illustration, motivational slogans, and sentimental
decorative copy. No sexual imagery or graphic gore.

## Canvas and delivery

| Item | Requirement |
| --- | --- |
| Card image | `infuser-art/card-hero.webp`, exactly **1024 × 1024 px**, square |
| Encoding | Static WebP, opaque RGB, sRGB; no transparency or animation |
| File size | Aim for 100–200 KiB; maximum **250 KiB** |
| Source | Retain the original editable or lossless artwork in the game's source project; do not upscale a small thumbnail |
| Handoff note | `infuser-art/README.md`: game title, one-sentence factual description of the scene, source/provenance, and whether the image is final or a concept |

Keep the source master separate from the small delivery file. If the generator cannot
produce the exact resolution, generate a square image at least 1024px wide and
resize once for export. Do not stretch a non-square image or add letterboxing.
Convert to sRGB, remove unnecessary metadata, and inspect the compressed export.
Strip the embedded color profile after conversion; the server assumes sRGB.
If compression damages the subject, simplify noisy detail rather than shrinking
below the required dimensions.

### Publish with the game

Place the delivery WebP at `infuser-art/card-hero.webp` in the project supplied
to `infuser publish`. The CLI also accepts it inside an already-built web export.
It validates the image and attaches it to the build, including for Godot exports
whose other resources are packed into a PCK. Exclude `infuser-art/*` from Godot's
resource export: the CLI uploads the delivery image separately. It skips the
other files in this reserved directory, keeping source masters and notes local.

Check `GET /api/publisher/capabilities`: `card_art.path` advertises this convention.
Custom publishers use the existing build file-upload route with that path. The
server validates dimensions, format, opacity, static encoding, and size before
publication. No new game SDK integration is involved.

Build-bundled artwork activates with its immutable game build; a queued update
keeps the old cover until activation. To add or replace only the cover, use the
[independent metadata API](game-metadata.md): upload the same delivery WebP with
`PUT /api/publisher/apps/APP/card-art`. This requires only the existing game
connection file, not its source or a new build. An explicit cover override survives
future builds until cleared with DELETE; then the active build cover is used again.
Missing art uses the abstract Library fallback. The Library returns an optional `card_art_url`, accessible only
to members who can see that game; hidden preview art is admin-only as well.

Only this square cover is supported. Do not submit a separate About-window
banner or stretch the cover into one.

## Compose for the real card

Infuser will render the art edge to edge with **centered `cover` cropping**. The
current cards are about 0.95:1 width-to-height; a narrow phone can reach about
0.75:1 because of the card's minimum height. A square source can therefore lose
roughly 13% from **each side**. Corners are rounded by the interface.

**Choose the subject's position freely within the crop-safe area.** There is no
required focal coordinate, pose, direction, or repeated composition. Wide vistas,
asymmetric subjects, and object-led images should not all look like the same
centered portrait. Keep the identifying feature visible through both cropping
and the actual overlays described below.

| Region | How to use it |
| --- | --- |
| Outer 15% on left and right of the source | Expendable scenery and atmosphere. No essential face, object, or defining feature here. |
| Top of the rendered card | Quieter supporting scenery behind one mode line and one horizontally scrollable tag row. At the smallest card, allow roughly the top quarter for metadata and inset. |
| Lower right of the rendered card | Supporting scenery behind the compact title/author/Play dock. The title is capped at two lines. On the smallest card, allow roughly the bottom 40% including the inset; the dock may span nearly its full width. |
| Remaining visible artwork | Place the identifying feature wherever it reads best. Avoid a tiny subject lost in scenery. Judge the actual overlay preview rather than aiming at a fixed percentage coordinate. |

Infuser limits the overlays rather than asking the artist to reserve ever more
space: tags stay in one scrollable row, titles show at most two lines, and player
names/reaction counts live in the existing dropdown. The dropdown also exposes
the full title on touch devices; the full heading remains available to assistive
technology. Expanded menus are temporary interactions, not permanent art masks.

Use the rest of the canvas to establish the world. One strong subject, one clear
direction of movement or depth, and restrained secondary detail usually work
better than a collage. Separate the subject from the background through value,
color, or silhouette so it survives a thumbnail view.

**The quiet regions must still be part of the scene.** Continue water, sky,
terrain, walls, or soft atmospheric shapes through them. Do not draw an empty
black panel, a diagonal cutout, a heavy baked fade, or a blank text reservation.
Infuser applies its own scrim and content-sized smoked-glass dock. Let the image
remain a complete illustration when viewed without UI.

Avoid a brilliant hotspot or sharp pattern beneath the lower-right controls.
Use lighting to lead toward the subject, not toward an otherwise empty corner.
Do not rely on a precise crop offset, per-game CSS, or a different mobile image
to rescue the composition.

## Leave interface elements out

Do not bake in a game title, logo, tagline, author name, avatar, rating, counter,
badge, button, border, rounded corner, screen bezel, or watermark. Infuser renders
its own text and controls. Avoid tiny lettering inside the scene as well.

Use original or appropriately licensed artwork. Preserve provenance in the
handoff note; identify generated art as generated art. A real gameplay screenshot
must meet the exceptional-cover standard above as well as every delivery requirement.
Do not describe an invented illustration as a gameplay capture.

## Review before handing off

1. Confirm the export's dimensions, color space, opacity, and file size.
2. Inspect it at about **96 × 96 px**. The subject and game identity should still
   be clear without reading a title. Remove visual noise that competes with them.
3. Preview centered-cover crops at **368 × 388 px** and **284 × 380 px**, plus a
   larger **600 × 632 px** card. Important content must survive all three.
4. Place the actual Library overlay over each crop: top metadata and the
   bottom-right title/author/Play dock. Check a long title and many tags too:
   the title must remain capped at two lines and tags at one scrollable row.
   If the standard controls obscure the subject, adjust the composition. Report
   overflowing Infuser controls as a platform issue; do not edit Infuser's UI.
5. Check the finished WebP, not only the source. Look for muddy edges, banding,
   compression ringing, accidental text, malformed objects, or baked-in patterns.

Keep the review captures separate from the submitted artwork. When reviewing a
new composition, first test the image against the shared layout; do not change
the card's crop or controls to hide an art problem.

## Using this guide with an image generator

Supply this complete document plus the game's title and a short factual brief
derived from the project. Game screenshots may be supplied as identity references
when available. Request **one square artwork image**, not a presentation sheet,
diagram, card mockup, collection, or image containing this document's instructions.
The agent handles WebP conversion and file verification after generation.
