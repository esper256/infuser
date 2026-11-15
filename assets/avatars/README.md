# Profile pictures

Thirty-one original AI-generated pictures for Infuser's profile picker, generated
for this project rather than sourced from a stock library or an existing game
franchise. The collection mixes characters with object-only gaming artwork.

- Original set: Circuit Fox, Orbit Racer, Moss Knight, Storm Drake, Deepwave.
- Masculine character portraits: Neon Runner, Bass Pilot, Frost Scout, Forge Rider.
- Object-only designs: Pulse Pad (controller), Prism Core (glass prism), Photon
  Loop (light sculpture), Pocket Arcade (cabinet), Critical Gem (crystal die),
  Ion Racer (racing craft).
- More subjects: Event Horizon (black hole), Tiny World (terrarium), Midnight
  Ramen (food), Mixtape (music), Kickflip (skateboarding), Jade Beetle (wildlife).
- Blocky 3D animals: Voxel Axolotl, Blocky Capybara, Bamboo Bandit (red panda),
  Frost Owl, Lime Frog.
- 2D pixel animals: Pixel Panther (black cat), Moon Wolf, Loot Raccoon,
  Byte Gecko, Ember Raven.

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
  for size in 128 256; do
    case "$name" in
      pixel-panther|moon-wolf|loot-raccoon|byte-gecko|ember-raven)
        magick "$name.png" -filter point -resize "${size}x${size}" -strip \
          -define webp:lossless=true "$name-$size.webp" ;;
      *) magick "$name.png" -resize "${size}x${size}" -quality 86 -strip "$name-$size.webp" ;;
    esac
  done
done
```

Pixel-art source exports and WebP sizes use nearest-neighbor scaling. Their WebP
exports are lossless to avoid introducing blur or ringing along pixel edges.
All source exports are opaque; the circle is a CSS crop, not baked transparency.
