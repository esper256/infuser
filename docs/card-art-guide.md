# Infuser card hero art

**Draft 0.1 — artwork preparation and review.** This guide specifies the image
for a Library card. Automatic registration of game artwork is not implemented
yet: preparing these files does not install them in the live Library. Do not add
invented artwork fields to `infuser.game.json` or call an undocumented API.

## What to make

Create one compelling, full-bleed image that identifies **this game** at a glance.
It sits behind Infuser's title, creator portrait, tags, and compact Play controls.
The image is artwork, not a screenshot of a finished card.

Start from the actual game: inspect its environments, characters, objects,
mechanics, screenshots, and existing art. Pick one recognizable subject or moment.
Keep its defining shapes and palette. An environment, vehicle, object, or abstract
pattern can lead the image; a character is not required. Do not invent a mascot,
mode, location, or feature to make the game appear more elaborate than it is.

Aim for confident, polished game key art with a strong silhouette, clear depth,
and a few well-chosen highlights. Preserve the game's own visual language: pixel
art, low-poly, illustrated, graphic, and other deliberate styles are welcome.
Infuser's dark interface is the frame, not a requirement to turn every game into
a neon city. Bright, warm, or daylight scenes can work too.

The audience includes young game makers. Be aspirational without being childish:
no nursery-like mascot treatment, patronizing depictions of children, corporate
stock illustration, motivational slogans, or sentimental decorative copy. Keep
the image appropriate for roughly ages 10–16; no sexual imagery or graphic gore.

## Canvas and delivery

| Item | Requirement |
| --- | --- |
| Card image | `infuser-art/card-hero.webp`, exactly **1024 × 1024 px**, square |
| Encoding | Static WebP, opaque RGB, sRGB; no transparency or animation |
| File size | Aim for 100–200 KiB; maximum **250 KiB** |
| Source | Retain the original editable or lossless artwork in the game's source project; do not upscale a small thumbnail |
| Handoff note | `infuser-art/README.md`: game title, one-sentence factual description of the scene, source/provenance, and whether the image is final or a concept |

These are preparation filenames, not an active manifest or upload contract. Keep
the source master separate from the small delivery file. If the generator cannot
produce the exact resolution, generate a square image at least 1024px wide and
resize once for export. Do not stretch a non-square image or add letterboxing.
Convert to sRGB, remove unnecessary metadata, and inspect the compressed export.
If compression damages the subject, simplify noisy detail rather than shrinking
below the required dimensions.

A larger About-window illustration will be a separate asset with its own future
layout. Do not submit one yet, and do not assume this square image can simply be
stretched into a wide banner.

## Compose for the real card

Infuser will render the art edge to edge with **centered `cover` cropping**. The
current cards are about 0.95:1 width-to-height; a narrow phone can reach about
0.75:1 because of the card's minimum height. A square source can therefore lose
roughly 13% from **each side**. Corners are rounded by the interface.

Coordinates below are percentages of the **source image**, measured from the
top-left. They are composition guides, not visible marks to paint into the art.

| Region | How to use it |
| --- | --- |
| Outer 15% on left and right | Expendable scenery and atmosphere. No essential face, object, or defining feature here. |
| Main subject: x 25–70%, y 25–62% | Keep the identifying silhouette or key feature here. A useful focal point is around x 42%, y 43%. Avoid a tiny subject lost in scenery. |
| Top 22% | Quieter background for mode/category chips. No crucial face or high-detail action behind them. |
| Lower-right: x 35–100%, y 65–100% | Low-detail supporting scenery. The title, author portrait, and Play dock can cover this area. Longer titles and narrow screens make the dock larger. |

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
is acceptable if it meets the composition requirements and has its HUD removed.
Do not describe an invented illustration as a gameplay capture.

## Review before handing off

1. Confirm the export's dimensions, color space, opacity, and file size.
2. Inspect it at about **96 × 96 px**. The subject and game identity should still
   be clear without reading a title. Remove visual noise that competes with them.
3. Preview centered-cover crops at **368 × 388 px** and **284 × 380 px**, plus a
   larger **600 × 632 px** card. Important content must survive all three.
4. Place the actual Library overlay over each crop: top metadata and the
   bottom-right title/author/Play dock. Check a long wrapped title and several
   tags too. A nominal safe region cannot guarantee every possible text length.
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

For a **guide-only concept test**, supply this document and the game title only,
without reference art or additional style instructions. If no gameplay facts are
provided, infer one visual interpretation of the title and record the result as
**unverified concept art**, not a production-ready depiction of that game. Keep
the exact prompt with the review so another agent can reproduce the experiment.

If the first render fails the crop or overlay checks, record the specific failure
and improve this guide before rerunning a guide-only test. Do not silently add
private corrective art direction that the next game-making agent will not see.
