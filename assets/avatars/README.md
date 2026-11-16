# Profile pictures

Audience: Infuser developers.

Eighty-one original AI-generated pictures for Infuser's profile picker, generated
for this project rather than sourced from a stock library or an existing game
franchise. The collection mixes characters with object-only gaming artwork.

- Original set: Circuit Fox, Orbit Racer, Moss Knight, Storm Drake, Deepwave.
- Character portraits: Neon Runner, Bass Pilot, Frost Scout, Forge Rider.
- Object-only designs: Pulse Pad (controller), Prism Core (glass prism), Photon
  Loop (light sculpture), Pocket Arcade (cabinet), Critical Gem (crystal die),
  Ion Racer (racing craft).
- More subjects: Event Horizon (black hole), Tiny World (terrarium), Midnight
  Ramen (food), Mixtape (music), Kickflip (skateboarding), Jade Beetle (wildlife).
- Blocky 3D animals: Voxel Axolotl, Blocky Capybara, Bamboo Bandit (red panda),
  Frost Owl, Lime Frog.
- 2D pixel animals: Pixel Panther (black cat), Moon Wolf, Loot Raccoon,
  Byte Gecko, Ember Raven.
- Sports and racing: Soccer Comet (soccer ball), Hoop Hero (basketball), Turbo
  Kart (go-kart).
- Dinosaurs and sea life: Jurassic Grin (T-rex), Reef Shark.
- Building and science: Mech Buddy (robot), Brick Rocket (toy construction),
  Lab Spark (laboratory flask).
- Playful fantasy and food: Treasure Mimic (chest creature), Pizza Planet.
- More sports and vehicles: Home Run (baseball), Slapshot (ice hockey),
  Monster Truck.
- More creatures: Astro Sloth, Moon Moth, Ink Captain (octopus), Cobalt Cobra.
- Magic, humor, and music: Spell Hat, Ghost Toast, Power Chord (electric guitar).
- Explorers: Star Cartographer, Cloud Rider.
- More companions: Comet Otter, Aurora Lynx, Lantern Dragon, Nebula Jelly,
  Copper Kestrel.
- Adventure objects: Secret Key, Stardust Deck (handheld game console), Tideglass
  (glass seashell).
- Retro pixel gaming objects: Combo Stick, Turbo Cartridge, Save Disk, Mana Flask,
  Bonus Heart, Loot Chest, Portal Door, Rocket Boots, Neon Pinball, Checkpoint Flag.
- Tiny secret worlds: Rooftop Arcade, Moon Greenhouse, Lantern Treehouse,
  Deepsea Observatory, Sky Garden, Snow Radio, Midnight Bazaar, Crystal Refuge,
  Ember Workshop, Orbit Hideout.

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
      pixel-*|moon-wolf|loot-raccoon|byte-gecko|ember-raven)
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

The preceding twenty originals were generated in four aligned sheets of five, then
exported together into the existing square PNG and WebP sizes. They expand sports,
dinosaurs, construction, science, music, and fantasy themes with strong silhouettes for
the small circular badges.

The ten adventure additions were generated individually on 2026-11-16 and imported
as one batch. They mix capable explorer portraits, animal and fantasy companions,
and object-only designs. Like every option, they appear in the shared picker
without demographic labels. Sources are opaque 512px PNGs, with the same 128px and
256px WebP exports as the rest of the catalog.

The ten retro object additions were generated individually on 2026-11-16 and
imported as one batch. All are inanimate gaming objects, with no characters or
faces. Their `pixel-*` sources and exports use nearest-neighbor resizing; both
WebP sizes are lossless, preserving the artwork’s hard pixel edges.

The ten secret-world additions were generated individually on 2026-11-16 and
imported as one batch. They depict miniature imaginative hideouts without people
or faces, using distinct architectural silhouettes and luminous color palettes.
Opaque 512px PNG sources have the standard 128px and 256px WebP exports.
