# Lantern Lake card art

Audience: Infuser developers.

**Final cover illustration.** Amber and cyan player lights drift across a dark
abstract lake, surrounded by teal ripples and small gold collectibles.

Generated with the built-in image generation tool from an actual Lantern Lake
multiplayer gameplay capture. This is an illustration, not a gameplay screenshot.
The lossless generated master is `source/card-hero.png`; the complete initial
prompt is `source/prompt.txt`, followed by `source/composition-edit.txt`.

The first composition placed the players under the Library's lower-right dock.
The recorded edit moves them into the unobscured middle of the card, preserving
their actual geometric shapes and colors. No extra game characters or features
were invented. The production export is `card-hero.webp`: 1024 × 1024, opaque
sRGB WebP with metadata stripped. Only that delivery file is published.

Export command (ImageMagick):

```sh
magick infuser-art/source/card-hero.png -resize 1024x1024! -colorspace sRGB -alpha off -strip -quality 88 infuser-art/card-hero.webp
```
