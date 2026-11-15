# Profile pictures

Fifteen original AI-generated pictures for Infuser's profile picker, generated
for this project rather than sourced from a stock library or an existing game
franchise. The collection mixes characters with object-only gaming artwork.

- Original set: Circuit Fox, Orbit Racer, Moss Knight, Storm Drake, Deepwave.
- Masculine character portraits: Neon Runner, Bass Pilot, Frost Scout, Forge Rider.
- Object-only designs: Pulse Pad (controller), Prism Core (glass prism), Photon
  Loop (light sculpture), Pocket Arcade (cabinet), Critical Gem (crystal die),
  Ion Racer (racing craft).

Each `NAME.png` is a 512×512 square source export. The `NAME-128.webp` files serve
the 34–36px header, presence, and creator circles; `NAME-256.webp` serves the 88px
picker previews. Keep the square aspect ratio and use a circular CSS crop. Faces
and key features remain legible at small sizes; no text is baked into the art.

The daemon embeds both WebP sizes so release binaries need no separate asset
installation. The curated IDs and names live in `src/portal/avatars.rs`.
Regenerate the web exports from this directory using ImageMagick:

```sh
for source in *.png; do
  name="${source%.png}"
  magick "$name.png" -resize 128x128 -quality 86 "$name-128.webp"
  magick "$name.png" -resize 256x256 -quality 86 "$name-256.webp"
done
```
