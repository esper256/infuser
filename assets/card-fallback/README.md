# Pending artwork fallback

Audience: Infuser developers.

`generating-master.png` is the approved image generated with OpenAI image generation
from a rendered reference of Infuser's existing abstract loop fallback. It is a
shared Library status image, not game-specific art and not a published cover.

`generating.webp` is the opaque sRGB 1024 × 1024 production export (quality 86).
Regenerate the export with:

```sh
convert assets/card-fallback/generating-master.png -resize 1024x1024 -colorspace sRGB -strip -quality 86 assets/card-fallback/generating.webp
```

Use only when there is no active cover and an unfulfilled art prompt exists.
A real cover always wins. Cancelling or completing the request restores the
ordinary fallback if no cover remains. Never upload this as a game's own cover:
that would prevent a missing-cover art agent from finding the game.

The placard says “Generating...” as requested. This marks a pending request, not
an active worker, countdown, or guaranteed delivery time. Scheduling stays external.
