#!/usr/bin/env bash
# Rebuild deterministic raster exports from the reviewed generated masters.
set -euo pipefail
cd "$(dirname "$0")"
for variant in navy light dark; do
  for size in 256 512 1024; do
    magick "lockup-${variant}-master.png" -filter Lanczos -resize "${size}x${size}" "lockup-${variant}-${size}.png"
  done
done
for variant in light dark; do
  for scale in 1 2; do
    magick -size "$((320*scale))x$((80*scale))" xc:none \
      \( "lockup-${variant}-master.png" +gravity -crop 700x880+320+30 +repage -resize "$((60*scale))x$((76*scale))" \) -gravity west -geometry +0+0 -composite \
      \( "lockup-${variant}-master.png" +gravity -crop 560x130+350+950 +repage -resize "$((240*scale))x$((60*scale))" \) -gravity east -geometry +0+0 -composite \
      "header-${variant}-$((320*scale)).png"
  done
done
for size in 64 128 180 192 256 512; do
  magick lockup-light-master.png -crop 700x880+320+30 +repage -resize "$((size*90/100))x$((size*90/100))" -gravity center -background none -extent "${size}x${size}" "icon-${size}.png"
done
for size in 16 32 48; do
  magick lockup-light-master.png -crop 620x620+310+310 +repage -resize "${size}x${size}" "favicon-${size}.png"
done
magick favicon-16.png favicon-32.png favicon-48.png favicon.ico
magick icon-180.png -background '#071019' -alpha remove -alpha off apple-touch-icon.png
# A local review sheet, with alpha composited onto the intended surfaces.
magick \( lockup-light-512.png -background '#0c1017' -alpha remove \) \
       \( lockup-dark-512.png -background '#f5f1e8' -alpha remove \) +append preview-backgrounds.png

# Center this crop on the visible wordmark (master x=630), not the vapor bounds.
# Landing artwork is embedded in index.html because the portal serves a fixed asset list.
magick lockup-light-master.png -crop 760x1100+250+10 +repage -resize 760x1100 -quality 90 landing-logo.webp
magick header-light-640.png -quality 90 header-light.webp

# Split the Library identity from the original PNG, never the lossy header WebP.
magick lockup-light-master.png -crop 700x880+320+30 +repage header-icon-master.png
# Opaque launcher icons. Android masks purpose "any" onto a white plate and
# shrinks it. Match apple-touch-icon.png (#071019) across the whole square.
# Keep the glyph inside the centered safe circle (diameter 80% of the canvas)
# and scale it until the farthest kept pixel sits on that circle. A 3% alpha
# threshold drops the master's near-zero green fringe and keeps the vapor.
mask_geom=$(magick header-icon-master.png -alpha extract -threshold 3% -format '%@' info:)
mask_radius=$(magick header-icon-master.png -alpha extract -threshold 3% -trim +repage \
  -fx 'u>0.5?hypot(i-(w-1)/2,j-(h-1)/2)/w:0' -format '%[fx:maxima*w]' info:)
mask_trim_w=$(magick header-icon-master.png -alpha extract -threshold 3% -trim +repage -format '%w' info:)
mask_trim_h=$(magick header-icon-master.png -alpha extract -threshold 3% -trim +repage -format '%h' info:)
for size in 192 512; do
  mask_w=$(magick xc: -format "%[fx:floor(${mask_trim_w}*0.4*${size}/${mask_radius})]" info:)
  mask_h=$(magick xc: -format "%[fx:floor(${mask_w}*${mask_trim_h}/${mask_trim_w})]" info:)
  magick -size "${size}x${size}" 'xc:#071019' \
    \( header-icon-master.png -crop "${mask_geom}" +repage -filter Lanczos -resize "${mask_w}x${mask_h}!" \) \
    -gravity center -composite -background '#071019' -alpha off -depth 8 \
    "icon-maskable-${size}.png"
done
magick lockup-light-master.png -crop 560x130+350+950 +repage header-wordmark-master.png
magick header-icon-master.png -resize 140x176 -define webp:lossless=true header-icon.webp
magick header-wordmark-master.png -resize 480x -define webp:lossless=true header-wordmark.webp
